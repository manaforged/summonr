#!/usr/bin/env python3
import glob
import json
import os
import statistics
import sys
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
METRICS = ("footprint_kb", "parent_peak_kb", "tree_peak_kb", "children_peak", "children_tail", "starts")


def load(path):
    with open(path, "rb") as source:
        return tomllib.load(source)


def scenarios(data, names):
    listed = data["scenario"]
    if not names:
        return listed
    known = {s["name"]: s for s in listed}
    missing = [n for n in names if n not in known]
    if missing:
        sys.exit(f"unknown scenario: {' '.join(missing)}")
    return [known[n] for n in names]


def find(data, name):
    return scenarios(data, [name])[0]


def plan(data, names):
    runs = data["defaults"]["runs"]
    for s in scenarios(data, names):
        print(s["name"], s.get("runs", runs), 1 if s.get("warm") else 0)


def script(data, name, out):
    with open(out, "w") as sink:
        json.dump(find(data, name)["script"], sink)


def value(item):
    if isinstance(item, bool):
        return "true" if item else "false"
    if isinstance(item, (int, float)):
        return str(item)
    if isinstance(item, dict):
        return "{ " + ", ".join(f"{k} = {value(v)}" for k, v in item.items()) + " }"
    return json.dumps(str(item))


def config(data, name, home, rundir, port, fixture):
    s = find(data, name)
    lines = [
        "[endpoint.replay]",
        f'base_url = "http://127.0.0.1:{port}/v1"',
        'model = "replay"',
        'api_key = "replay"',
        "context_window = 262144",
        "",
        "[modules]",
        f"mcp = {value(bool(s['mcp']))}",
    ]
    for n in range(1, s.get("servers", 0) + 1):
        env = {"MCP_FIXTURE_SERVE": "1", "MCP_FIXTURE_LOG": os.path.join(rundir, f"fixture{n}.log"), "MCP_FIXTURE_STARTS": os.path.join(rundir, "starts")}
        lines += ["", f"[mcp.fixture{n}]", f"command = {value(fixture)}", f"env = {value(env)}"]
        lines += [f"{k} = {value(v)}" for k, v in s.get("server_extra", {}).items()]
    with open(os.path.join(home, "config.toml"), "w") as sink:
        sink.write("\n".join(lines) + "\n")


def footprint(home, since):
    peak = 0
    for path in glob.glob(os.path.join(home, "sessions", "**", "session.jsonl"), recursive=True):
        if os.path.getmtime(path) < since:
            continue
        for line in open(path, errors="replace"):
            try:
                row = json.loads(line)
            except ValueError:
                continue
            if isinstance(row, dict):
                peak = max(peak, row.get("footprint_kb") or row.get("rss_kb") or 0)
    return peak


def samples(path):
    rows = []
    for line in open(path, errors="replace"):
        parts = line.split()
        if len(parts) >= 3 and all(p.isdigit() for p in parts[:3]):
            rows.append(tuple(int(p) for p in parts[:3]))
    return rows


def measure(data, rundir, home):
    rows = samples(os.path.join(rundir, "rss"))
    fraction = data["defaults"]["tail_fraction"]
    tail = rows[-max(1, int(len(rows) * fraction)):] if rows else []
    starts = os.path.join(rundir, "starts")
    result = {
        "footprint_kb": footprint(home, os.path.getmtime(os.path.join(rundir, "started"))),
        "parent_peak_kb": max((r[0] for r in rows), default=0),
        "tree_peak_kb": max((r[1] for r in rows), default=0),
        "children_peak": max((r[2] for r in rows), default=0),
        "children_tail": max((r[2] for r in tail), default=0),
        "starts": sum(1 for _ in open(starts)) if os.path.exists(starts) else 0,
        "samples": len(rows),
    }
    with open(os.path.join(rundir, "metrics.json"), "w") as sink:
        json.dump(result, sink)


def medians(data, out, names):
    table = {}
    for s in scenarios(data, names):
        runs = []
        for path in sorted(glob.glob(os.path.join(out, s["name"], "run-*", "metrics.json"))):
            with open(path) as source:
                runs.append(json.load(source))
        if not runs:
            sys.exit(f"no runs for {s['name']} in {out}")
        table[s["name"]] = {m: statistics.median(r[m] for r in runs) for m in METRICS}
        table[s["name"]]["runs"] = [{m: r[m] for m in METRICS} for r in runs]
    return table


def thresholds_path(data):
    return os.path.join(HERE, data["defaults"]["thresholds"].format(platform=sys.platform))


def capture(data, out, names):
    table = medians(data, out, names)
    path = thresholds_path(data)
    kept = load(path) if os.path.exists(path) else {}
    for name, row in table.items():
        kept[name] = {m: row[m] for m in METRICS}
    lines = []
    for name in sorted(kept):
        lines.append(f'["{name}"]')
        lines += [f"{m} = {value(v)}" for m, v in kept[name].items()]
        lines.append("")
    with open(path, "w") as sink:
        sink.write("\n".join(lines))
    with open(os.path.join(out, "results.json"), "w") as sink:
        json.dump({"mode": "capture", "scenarios": table}, sink, indent=2)
    print(f"wrote {path}")


def verdict(rule, metric, current, gate, captured, table, band):
    if rule == "threshold":
        limit = captured.get(metric)
        return (None, None) if limit is None else (current <= limit * (1 + band), limit)
    if rule == "match":
        ref = table.get(gate["ref"], {}).get(metric)
        if ref is None:
            return None, None
        limit = ref * (1 + band) + gate.get("allow_kb", 0)
        return current <= limit, round(limit)
    if rule == "max":
        return current <= gate["value"], gate["value"]
    if rule == "equal":
        return current == gate["value"], gate["value"]
    return None, None


def check(data, out, names):
    table = medians(data, out, names)
    path = thresholds_path(data)
    if not os.path.exists(path):
        sys.exit(f"missing {path}; run capture first")
    base = load(path)
    lines, failed = [], False
    for s in scenarios(data, names):
        row = table[s["name"]]
        captured = base.get(s["name"], {})
        for gate in s.get("gates", []):
            metric, rule = gate["metric"], gate["rule"]
            band = gate.get("band", data["bands"].get(metric, 0.0))
            reference = dict(base)
            reference.update(table)
            ok, limit = verdict(rule, metric, row[metric], gate, captured, reference, band)
            if rule == "record":
                word = "INFO"
            elif ok is None:
                word, failed = "FAIL", True
            else:
                word = "PASS" if ok else "FAIL"
                failed = failed or not ok
            lines.append({"verdict": word, "scenario": s["name"], "metric": metric, "rule": rule, "value": row[metric], "threshold": limit})
            print(word, s["name"], metric, row[metric], limit)
    with open(os.path.join(out, "results.json"), "w") as sink:
        json.dump({"mode": "check", "scenarios": table, "gates": lines}, sink, indent=2)
    sys.exit(1 if failed else 0)


def main():
    command, rest = sys.argv[1], sys.argv[2:]
    data = load(os.path.join(HERE, "scenarios.toml"))
    if command == "plan":
        plan(data, rest)
    elif command == "script":
        script(data, *rest)
    elif command == "config":
        config(data, *rest)
    elif command == "measure":
        measure(data, *rest)
    elif command == "capture":
        capture(data, rest[0], rest[1:])
    elif command == "check":
        check(data, rest[0], rest[1:])
    else:
        sys.exit(f"unknown command {command}")


if __name__ == "__main__":
    main()
