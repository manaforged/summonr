import enum
import fcntl
import json
import os
import re
import shlex
import shutil
import signal
import subprocess
import sys
import threading
import time
import tomllib
from pathlib import Path

from store import Store

DB = Path(".summonr/research/run.sqlite")
GRACE_SECS = 20
SESSION_SECS = 14400
SHORT_SESSION_SECS = 60
AGENT = ["summonr", "-p", "--name", "{session}"]
VERDICT = r"(?P<gain>[+-]?\d+(?:\.\d+)?)[^\[\n]*\[(?P<low>[+-]?\d+(?:\.\d+)?),\s*(?P<high>[+-]?\d+(?:\.\d+)?)\]"
FIELD = re.compile(r"\{(\w+)\}")
CLI = Path(__file__).resolve().parents[1] / "bin" / "researchr"
RULES = """Rules:
1. Change mechanisms, not constants: each edit removes a cost a measurement shows.
2. Measure only the cases your edit touches in the inner loop.
3. Two strikes: if the target is still worse after two builds of one mechanism, put your file back to its last good state and take the next mechanism.
4. Submit each measured win at once and keep working while the judge runs."""
BRIEF = """You climb one measured goal in lane {lane}, for as many iterations as this session allows. After each result go straight to the next edit; one measurement per session wastes the run.

Goal: {goal}
Your file: {file}. Edit only this file and {status}.
Status file: {status}. Rewrite it after each result: what your file holds, the last result, the next action. A new session of this lane resumes from it.

{rules}

Bank a win: `{researchr} submit "<one line: the mechanism you changed>"`, with background true. The harness runs the judge on your file and records the verdict; the judge alone keeps or rejects. Never run the judge yourself and never report your own numbers.

Other commands (long ones with background true, then wait on the job):
{commands}

Inbox: {inbox}. Read its new lines after each measurement. A kept win on your lane moved the best: bring your file onto it with the seed command before your next build. A win from another lane: port the idea by hand when your file has that code.

Judged submissions on this lane, newest last:
{log}

Status from the last session:
{resume}"""


class Kind(enum.StrEnum):
    KEPT = "kept"
    REJECTED = "rejected"
    ABANDONED = "abandoned"


class Env(enum.StrEnum):
    SPEC = "RESEARCHR_SPEC"
    ROOT = "RESEARCHR_ROOT"
    LANE = "RESEARCHR_LANE"
    RUNNER = "RESEARCHR_RUNNER"
    SESSION = "RESEARCHR_SESSION"


def spec(path):
    data = tomllib.loads(Path(path).read_text())
    if not isinstance(data.get("goal"), str) or not data["goal"].strip():
        raise ValueError("spec needs a goal")
    if not isinstance(data.get("target", {}).get("judge"), str):
        raise ValueError("spec needs [target] judge, a command that exits 0 to keep the file")
    if not data.get("lane"):
        raise ValueError("spec needs at least one [[lane]] with a file")
    names = {lane.get("name") for lane in data["lane"]}
    for lane in data["lane"]:
        if not isinstance(lane.get("name"), str) or not isinstance(lane.get("file"), str):
            raise ValueError("every lane needs a name and a file template")
        if type(lane.get("runners", 1)) is not int or lane.get("runners", 1) < 0:
            raise ValueError(f"lane {lane['name']}: runners must be a whole number")
        if not set(lane.get("ports", [])) <= names:
            raise ValueError(f"lane {lane['name']}: ports names a lane the spec does not have")
    return data


def fill(template, values):
    return FIELD.sub(lambda found: str(values.get(found[1], found[0])), template)


def folder(root):
    return root / DB.parent


def inbox(root, lane):
    return folder(root) / "inbox" / f"{lane}.log"


def log(store, lane, lines):
    rows = store.db.execute("select * from attempt where lane=? order by id desc limit ?", (lane, lines)).fetchall()
    return "\n".join(f"- {row['runner']} {row['kind']}" + (f" {row['gain']:+g} [{row['low']:+g}, {row['high']:+g}]" if row["low"] is not None else "")
                     + f": {row['mechanism']}" for row in reversed(rows)) or "- none yet"


def verdict(data, text):
    found = list(re.finditer(data.get("verdict") or VERDICT, text))
    return tuple(float(found[-1][key]) for key in ("gain", "low", "high")) if found else (None, None, None)


def run(command, root, out, timeout=None, env=None):
    with open(out, "a") as sink:
        child = subprocess.Popen(command, shell=isinstance(command, str), cwd=root, stdin=subprocess.DEVNULL,
                                 stdout=sink, stderr=subprocess.STDOUT, start_new_session=True, env=env)
        try:
            return child.wait(timeout)
        except BaseException:
            stop(child)
            sink.write(f"\nstopped after {timeout} s\n")
            return None


def stop(child):
    for signal_, grace in ((signal.SIGTERM, GRACE_SECS), (signal.SIGKILL, None)):
        try:
            os.killpg(child.pid, signal_)
            child.wait(grace)
            break
        except ProcessLookupError:
            break
        except subprocess.TimeoutExpired:
            continue


def record(store, values):
    store.db.execute("begin immediate")
    store.db.execute("insert into attempt(lane,runner,session,kind,gain,low,high,mechanism,file,log,at) values(?,?,?,?,?,?,?,?,?,?,?)", values)
    store.db.commit()


def lane_of(data, name):
    return next(lane for lane in data["lane"] if lane["name"] == name)


def values_of(lane, runner, session, root):
    values = {**{k: v for k, v in lane.items() if isinstance(v, str)}, "runner": runner, "lane": lane["name"], "session": session}
    values["file"] = (root / fill(lane["file"], values)).relative_to(root)
    values["status"] = (folder(root) / "status" / f"{runner}.txt").relative_to(root)
    return values


def brief(store, data, root, lane, values):
    rules = Path(data["prompt"]).expanduser().read_text() if data.get("prompt") else RULES
    commands = "\n".join(f"- {verb}: `{fill(template, values)}`" for verb, template in data["target"].items() if verb != "judge")
    status = root / values["status"]
    return BRIEF.format(researchr=CLI, goal=data["goal"], lane=lane["name"], file=values["file"], status=values["status"], rules=rules.strip(),
                        commands=commands or "- none", inbox=inbox(root, lane["name"]).relative_to(root),
                        log=log(store, lane["name"], data.get("log_lines", 30)),
                        resume=status.read_text().strip() if status.exists() else "- first session")


def session(store, data, root, lane, runner, n, spec_path):
    name = f"{runner}-s{n}"
    values = values_of(lane, runner, name, root)
    out = folder(root) / "sessions" / name
    out.mkdir(parents=True, exist_ok=True)
    (root / values["status"]).parent.mkdir(parents=True, exist_ok=True)
    if n == 1 and data["target"].get("seed"):
        run(fill(data["target"]["seed"], values), root, out / "seed.log")
    env = {**os.environ, Env.SPEC: str(Path(spec_path).resolve()), Env.ROOT: str(root), Env.LANE: lane["name"],
           Env.RUNNER: runner, Env.SESSION: name}
    argv = [fill(part, values) for part in data.get("agent", AGENT)]
    began = time.time()
    run([*argv, brief(store, data, root, lane, values)], root, out / "agent.log", data.get("session_secs", SESSION_SECS), env)
    return time.time() - began


def submit(mechanism):
    root = Path(os.environ[Env.ROOT])
    data = spec(os.environ[Env.SPEC])
    lane = lane_of(data, os.environ[Env.LANE])
    runner, name = os.environ[Env.RUNNER], os.environ[Env.SESSION]
    values = values_of(lane, runner, name, root)
    store = Store(root / DB)
    try:
        n = store.db.execute("select count(*) from attempt where runner=?", (runner,)).fetchone()[0] + 1
        out = folder(root) / "attempts" / f"{runner}-a{n}"
        out.mkdir(parents=True, exist_ok=True)
        snapshot = out / Path(values["file"]).name
        shutil.copyfile(root / values["file"], snapshot)
        values.update(snapshot=snapshot.relative_to(root), mechanism=shlex.quote(mechanism))
        with open(folder(root) / f"{lane['name']}.lock", "a") as turn:
            fcntl.flock(turn, fcntl.LOCK_EX)
            code = run(fill(data["target"]["judge"], values), root, out / "judge.log")
        kind = Kind.KEPT if code == 0 else Kind.REJECTED
        text = (out / "judge.log").read_text(errors="replace")
        gain, low, high = verdict(data, text)
        record(store, (lane["name"], runner, name, kind, gain, low, high, mechanism, str(values["file"]), str(out / "judge.log"), time.time()))
        if kind == Kind.KEPT:
            line = f"{time.strftime('%H:%M')} {lane['name']} kept {runner}: {mechanism}" + (f" {gain:+g} [{low:+g}, {high:+g}]" if low is not None else "")
            for target in (lane["name"], *lane.get("ports", [])):
                inbox(root, target).parent.mkdir(parents=True, exist_ok=True)
                with open(inbox(root, target), "a") as sink:
                    sink.write(line + "\n")
        return {"kind": kind, "gain": gain, "low": low, "high": high, "log": str((out / "judge.log").relative_to(root)),
                "tail": text.strip().splitlines()[-5:]}
    finally:
        store.db.close()


def runner(path, data, root, lane, name, sessions, export, spec_path):
    store = Store(path)
    halt = folder(root) / "STOP"
    began = time.time()
    short = 0
    try:
        for n in range(1, sessions + 1):
            if halt.exists() and halt.stat().st_mtime > began:
                return
            lasted = session(store, data, root, lane, name, n, spec_path)
            try:
                export(store, f"{name}-s{n}", None)
            except (ValueError, OSError, subprocess.SubprocessError):
                pass
            short = short + 1 if lasted < SHORT_SESSION_SECS else 0
            if short >= 2:
                print(json.dumps({"runner": name, "error": f"two sessions ended within {SHORT_SESSION_SECS} s; see {folder(root) / 'sessions'}"}), file=sys.stderr, flush=True)
                return
    finally:
        store.db.close()


def climb(spec_path, root, sessions, export):
    data = spec(spec_path)
    path = root / DB
    Store(path, create=True).db.close()
    threads = [threading.Thread(target=runner, args=(path, data, root, lane, f"{lane['name']}-{k}", sessions, export, spec_path))
               for lane in data["lane"] for k in range(1, lane.get("runners", 1) + 1)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    return status(Store(path), 10)


def status(store, recent):
    lanes = store.db.execute("""select lane, sum(kind='kept') as kept, sum(kind='rejected') as rejected,
                                       sum(kind='abandoned') as abandoned from attempt group by lane""").fetchall()
    rows = store.db.execute("select lane, session, kind, gain, low, high, mechanism from attempt order by id desc limit ?", (recent,)).fetchall()
    return {"lanes": [dict(row) for row in lanes], "recent": [dict(row) for row in rows]}
