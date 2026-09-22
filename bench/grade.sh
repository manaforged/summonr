#!/usr/bin/env bash
set -euo pipefail

tasks_dir=${1:-$(cd "$(dirname "$0")" && pwd)/tasks}
summonr=${2:-$(cd "$(dirname "$0")/.." && pwd)/target/release/summonr}
[ -x "$summonr" ] || summonr=${summonr/release/debug}
tasks_dir=$(cd "$tasks_dir" && pwd)
summonr=$(cd "$(dirname "$summonr")" && pwd)/$(basename "$summonr")
repo=$(cd "$(dirname "$0")/.." && pwd)
repeats=${AB_REPEATS:-3}
concurrent=${AB_CONCURRENT:-8}
port=${VS_PORT:-8095}
model=${BENCH_MODEL:-deepseek-v4-flash}
task_timeout=${TASK_TIMEOUT:-600}
max_tokens=${BENCH_MAX_TOKENS:-384000}
out=$(mktemp -d)/grade
works=$(mktemp -d)
mkdir -p "$out"
: > "$out/proxy.jsonl"

if [ -z "${DEEPSEEK_API_KEY:-}" ]; then
    DEEPSEEK_API_KEY=$(awk '/^\[endpoint\.deepseek\]/{f=1;next} /^\[/{f=0} f&&/^api_key/{gsub(/[" ]/, "", $3); print $3; exit}' "$HOME/.summonr/config.toml" 2>/dev/null)
fi
[ -n "$DEEPSEEK_API_KEY" ] || { echo "grade.sh: no DEEPSEEK_API_KEY and none in ~/.summonr/config.toml" >&2; exit 1; }

python3 "$repo/bench/proxy.py" --bind 127.0.0.1 --port "$port" \
    --log "$out/proxy.jsonl" --upstream https://api.deepseek.com/v1 &
proxy_pid=$!
disown
trap 'kill $proxy_pid 2>/dev/null || true; rm -f "${summonr_home:-}"/credentials.toml "${summonr_home:-}"/credentials.wrap; rm -rf "$works"' EXIT
sleep 0.5

run_pool() {
    for taskdir in "$tasks_dir"/*/; do
        local task=$(basename "$taskdir")
        for repeat in $(seq "$repeats"); do
            for label in summonr pi; do
                while [ "$(jobs -pr | wc -l)" -ge "$concurrent" ]; do sleep 0.3; done
                run_one "$label" "$out/$label-home" "$task" "$repeat" &
            done
        done
    done
    wait
}

run_one() {
    local label=$1 home=$2 task=$3 repeat=$4
    local taskdir="$tasks_dir/$task"
    local work="$works/$label-$task-$repeat"
    local prompt
    prompt=$(cat "$taskdir/task.txt")
    mkdir -p "$work"
    if [ -d "$taskdir/fixture" ]; then
        cp -R "$taskdir/fixture/." "$work/"
    else
        git -C "$repo" archive HEAD | tar -x -C "$work"
        rm -rf "$work"/bench/tasks*/
    fi
    git -C "$work" init -q
    git -C "$work" add -A
    git -C "$work" -c user.name=bench -c user.email=bench@localhost commit -qm baseline
    if [ "$label" = summonr ]; then
        (cd "$work" && exec env HOME="$home" SUMMONR_HOME="$home" \
            "$summonr" -e deepseek --max-secs "$task_timeout" --effort "${BENCH_EFFORT:-high}" -p "$prompt") \
            >"$out/$label-$task-$repeat.stdout" 2>"$out/$label-$task-$repeat.time" &
    else
        (cd "$work" && exec env HOME="$home" PI_CODING_AGENT_DIR="$home/pi" \
            DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" \
            pi --print --mode json --provider deepseek --model "$model" \
            --thinking "${BENCH_EFFORT:-high}" \
            --session-dir "$home/pi-sessions" "$prompt" </dev/null) \
            >"$out/$label-$task-$repeat.stdout" 2>"$out/$label-$task-$repeat.time" &
    fi
    local agent_pid=$!
    "$repo/bench/rsswatch.sh" "$agent_pid" "$out/$label-$task-$repeat.rss" "$task_timeout" &
    wait "$agent_pid" 2>/dev/null || true
    wait 2>/dev/null || true
    if [ -f "$taskdir/verify.sh" ]; then
        if (cd "$work" && bash "$tasks_dir/$task/verify.sh") >"$out/$label-$task-$repeat.verify" 2>&1; then
            echo pass > "$out/$label-$task-$repeat.grade"
        else
            echo fail > "$out/$label-$task-$repeat.grade"
        fi
    fi
    rm -rf "$work"
}

summonr_home="$out/summonr-home"
mkdir -p "$summonr_home"
for auth in credentials.toml credentials.wrap; do
    cp "$HOME/.summonr/$auth" "$summonr_home/" 2>/dev/null || true
done
cat > "$summonr_home/config.toml" <<EOF
[endpoint.deepseek]
base_url = "http://127.0.0.1:$port/summonr"
model = "$model"
context_window = 1048576
body = { max_tokens = $max_tokens }
EOF

pi_home="$out/pi-home"
mkdir -p "$pi_home/pi"
cat > "$pi_home/pi/models.json" <<EOF
{"providers": {"deepseek": {"baseUrl": "http://127.0.0.1:$port/pi", "modelOverrides": {"$model": {"maxTokens": $max_tokens}}}}}
EOF
for home in "$summonr_home" "$pi_home"; do
    printf '[user]\n\tname = bench\n\temail = bench@localhost\n' > "$home/.gitconfig"
done
{
    echo "model $model"
    echo "effort ${BENCH_EFFORT:-high}"
    echo "max_tokens $max_tokens"
    echo "repeats $repeats"
    echo "task_timeout $task_timeout"
    echo "tasks $tasks_dir"
    echo "pi $(pi --version 2>&1 | tail -1)"
    echo "summonr_sha256 $(shasum -a 256 "$summonr" | cut -d' ' -f1)"
    echo "summonr_describe $(git -C "$repo" describe --always --dirty)"
} > "$out/run.txt"

run_pool

SUMMONR_HOME="$summonr_home" "$summonr" profile --all > "$out/summonr.profile" || true
python3 - "$out" "$tasks_dir" <<'PY'
import json, sys, collections, glob, re, statistics, random, os

out, tasks_dir = sys.argv[1], sys.argv[2]
tasks = sorted(os.path.basename(p.rstrip("/")) for p in glob.glob(f"{tasks_dir}/*/"))

runs = collections.defaultdict(lambda: {"grade": None, "in": 0, "out": 0,
    "cached": 0, "latency": 0.0, "tools": collections.Counter(), "rss": None})

rows = collections.defaultdict(lambda: {"requests": 0, "in": 0, "out": 0,
    "cached": 0, "latency": 0.0, "tools": collections.Counter(), "runs": 0})
for line in open(f"{out}/proxy.jsonl"):
    rec = json.loads(line)
    row = rows[rec["label"]]
    row["requests"] += 1
    usage = rec.get("usage") or {}
    row["in"] += usage.get("prompt_tokens") or 0
    row["out"] += usage.get("completion_tokens") or 0
    row["cached"] += rec.get("cached_tokens") or 0
    row["latency"] += rec.get("latency") or 0
    for call in rec["request"].get("round_calls") or []:
        row["tools"][call.get("name") or "?"] += 1

grades = collections.defaultdict(dict)
for f in glob.glob(f"{out}/*.grade"):
    base = os.path.basename(f)[:-6]
    label, rest = base.split("-", 1)
    task, repeat = rest.rsplit("-", 1)
    grades[task][label] = grades[task].get(label, []) + [open(f).read().strip() == "pass"]

rss = collections.defaultdict(list)
for f in glob.glob(f"{out}/*.rss"):
    base = os.path.basename(f)[:-4]
    label = base.split("-")[0]
    samples = [line.split() for line in open(f, errors="replace")]
    samples = [(int(a), int(b)) for a, b, *_ in (s for s in samples if len(s) >= 2) if a.isdigit() and b.isdigit()]
    if samples:
        parent = [a for a, _ in samples]
        tree = [b for _, b in samples]
        rss[label].append((statistics.mean(parent), max(parent), statistics.mean(tree), max(tree)))

PRICE_IN, PRICE_OUT, PRICE_CACHED = 0.3, 1.2, 0.006

rates = {}
for label in ("summonr", "pi"):
    total = solved = 0
    for task in tasks:
        for g in grades.get(task, {}).get(label, []):
            total += 1
            solved += g
    rates[label] = (solved, total, solved / total if total else 0)

random.seed(7)
diffs = []
per_task = {t: {l: (sum(grades.get(t, {}).get(l, []) or [0]) / max(1, len(grades.get(t, {}).get(l, [])))) for l in ("summonr", "pi")} for t in tasks}
for _ in range(10000):
    sample = [random.choice(tasks) for _ in tasks]
    d = statistics.mean(per_task[t]["summonr"] for t in sample) - statistics.mean(per_task[t]["pi"] for t in sample)
    diffs.append(d)
diffs.sort()
lo, hi = diffs[250], diffs[9749]

for label in ("summonr", "pi"):
    row = rows.get(label, {"requests": 0, "in": 0, "out": 0, "cached": 0, "latency": 0.0, "tools": collections.Counter()})
    solved, total, rate = rates[label]
    cost = (row["in"] - row["cached"]) * PRICE_IN / 1e6 + row["cached"] * PRICE_CACHED / 1e6 + row["out"] * PRICE_OUT / 1e6
    calls = " · ".join(f"{n} {c}" for n, c in row["tools"].most_common())
    print(f"== {label} ==")
    print(f"resolution {solved}/{total} ({rate*100:.0f}%) · requests {row['requests']}")
    print(f"tokens    in {row['in']:,} (cached {row['cached']:,}) · uncached {row['in']-row['cached']:,} · out {row['out']:,} · latency {row['latency']:.0f}s")
    print(f"cost      ${cost:.3f} total · ${cost/solved:.3f}/solved" if solved else f"cost      ${cost:.3f} total · 0 solved")
    if rss.get(label):
        for name, offset in (("parent", 0), ("tree", 2)):
            means = [r[offset] for r in rss[label]]
            peaks = [r[offset + 1] for r in rss[label]]
            print(f"rss {name:6} mean {statistics.median(means)/1024:.0f} MB · peak median {statistics.median(peaks)/1024:.0f} MB · peak max {max(peaks)/1024:.0f} MB")
    print(f"calls     {calls}")
    print()

print(f"paired rate diff (summonr − pi): {lo*100:+.1f}% to {hi*100:+.1f}% (95% task-bootstrap CI)")
for task in tasks:
    s = sum(grades.get(task, {}).get("summonr", []))
    p = sum(grades.get(task, {}).get("pi", []))
    sn = len(grades.get(task, {}).get("summonr", []))
    pn = len(grades.get(task, {}).get("pi", []))
    flag = "  <—" if s * pn != p * sn else ""
    print(f"  {task:20} summonr {s}/{sn} · pi {p}/{pn}{flag}")
PY

echo "grade bench $out · model $model · repeats $repeats · tasks $tasks_dir · rss is ps RSS (not phys_footprint), parent PID and process tree"
cat "$out/run.txt"
cat "$out/summonr.profile"
