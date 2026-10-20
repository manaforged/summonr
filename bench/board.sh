#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")" && pwd)
run=${1:?usage: board.sh RUN_ID PAIR}
pair=${2:?usage: board.sh RUN_ID PAIR}
template=$root/pairs/$pair.yaml
[ -f "$template" ] || { echo "no pair $pair in $root/pairs" >&2; exit 1; }
export PATH=$PATH:$HOME/.local/bin PYTHONPATH=$root
export LOGINS=${BENCH_LOGINS:-$HOME} SUMMONR=${BENCH_SUMMONR:-$root/tools/summonr/bin/summonr}
export CODEX_AUTH_JSON_PATH=${CODEX_AUTH_JSON_PATH:-$LOGINS/.codex/auth.json}
export SUMMONR_SHA256=$(sha256sum "$SUMMONR" | cut -d' ' -f1)
want_harbor=$(awk '$1=="harbor"{print $2}' "$root/tools.lock")
got_harbor=$(harbor --version)
[ "$got_harbor" = "$want_harbor" ] || { echo "harbor $got_harbor, but tools.lock pins harbor $want_harbor: uv tool install harbor==$want_harbor" >&2; exit 1; }
jobs=$root/jobs
dir=$jobs/$run
if [ -f "$root/pairs/$pair.proxy" ]; then
  proxy() { awk -v k="$1" '$1==k{print $2}' "$root/pairs/$pair.proxy"; }
  . "$root/proxy-start.sh"
  start_proxy "$run" "$(proxy port)" "$(proxy upstream)" "$(proxy envfile)" "$(proxy keyenv)" $(awk '$1=="route"{print "--route", $2}' "$root/pairs/$pair.proxy")
  export BENCH_PROXY=http://host.docker.internal:$(proxy port) "$(proxy keyenv)=bench-proxy"
fi

fetch=$(mktemp -d)
trap 'rm -rf "$fetch"' EXIT
spec=$(python3 -c 'import re, sys; name, ref = re.search(r"\{name: ([^,}\s]+)(?:, ref: \"?([^,}\"\s]+))?", open(sys.argv[1]).read()).groups(); print(f"{name}@{ref}" if ref not in (None, "latest") else name)' "$template")
dataset=${spec##*/}
harbor download "$spec" -o "$fetch" > /dev/null
got=$(cd "$fetch/${dataset%@*}" && find . -type f -print0 | sort -z | xargs -0 sha256sum | sha256sum | cut -d' ' -f1)
want=$(awk -v key="$dataset" '$1==key{print $2}' "$root/dataset.lock")
[ "$got" = "$want" ] || { echo "dataset lock: $dataset wants $want, hub has $got" >&2; exit 1; }
bench="$(git -C "$root" rev-parse --short=12 HEAD)$([ -z "$(git -C "$root" status --porcelain -- .)" ] || echo -dirty)"

if [ -d "$dir" ]; then
  echo "resume $(date -u +%FT%TZ) bench $bench" >> "$dir/run.txt"
  python3 "$root/rerun.py" "$dir" "$root/logs/$run.jsonl" ${BENCH_RERUN:+--rerun}
  exec harbor job resume -p "$dir" --plugin rss:Rss
fi

[ -z "${BENCH_PREFLIGHT:-}" ] || export BENCH_TASKS=${BENCH_TASKS:-embedding-drift-monitor}
protocol=$root/protocols/$pair.md
if [ -z "${BENCH_TASKS:-}" ]; then
  [ -f "$protocol" ] || { echo "a full $pair run needs its protocol at $protocol" >&2; exit 1; }
  [ ! -f "$root/pairs/$pair.proxy" ] || [ -f "$root/pairs/$pair.settings" ] || { echo "a full $pair run needs pairs/$pair.settings: run BENCH_PREFLIGHT=1 ./board.sh RUN $pair and commit what it writes" >&2; exit 1; }
  case "$bench $(awk '$1=="summonr_commit"{print $2}' "$root/tools/versions.txt")" in
    *-dirty*) echo "a full $pair run needs a committed bench and summonr build: $bench" >&2; exit 1 ;;
  esac
fi
mkdir -p "$dir"
cp "$SUMMONR" "$dir/summonr"
export SUMMONR=$dir/summonr
python3 - "$template" "$root/tools.lock" "$dir/job.yaml" > "$dir/run.txt" <<'EOF'
import os, re, string, sys
pins = dict(line.split() for line in open(sys.argv[2]) if line.strip())
text = string.Template(open(sys.argv[1]).read()).substitute({**pins, **os.environ})
org = re.search(r"\{name: ([^/\s]+)/", text)[1]
tasks = [task if "/" in task else f"{org}/{task}" for task in os.environ.get("BENCH_TASKS", "").split()]
if os.environ.get("BENCH_PREFLIGHT"):
    text = re.sub(r"n_attempts: \d+", "n_attempts: 1", text)
    if cap := os.environ.get("BENCH_PREFLIGHT_TIMEOUT_MULT"):
        text = re.sub(r"(timeout_multiplier: [\d.]+\n)", rf"\1agent_timeout_multiplier: {cap}\n", text, count=1)
    text += f"tasks:\n  - {{path: {os.path.dirname(os.path.dirname(os.path.abspath(sys.argv[1])))}/probe/env-probe}}\n"
if tasks:
    entries = re.findall(r"\n  - \{name: [^}]*\}", text)
    assert len(entries) == 1, entries
    text = text.replace(entries[0], f"{entries[0][:-1]}, task_names: [{', '.join(tasks)}]}}")
    print("tasks", " ".join(tasks))
open(sys.argv[3], "w").write(text)
print("model", re.search(r"model_name: [^/\s]+/(\S+)", text)[1])
print("effort", re.search(r"reasoning_effort: (\w+)", text)[1])
print("repeats", re.search(r"n_attempts: (\d+)", text)[1])
print("timeout_mult", re.search(r"timeout_multiplier: ([\d.]+)", text)[1])
EOF
{
  echo "vendor $pair"
  echo "dataset $got"
  echo "summonr $(sha256sum "$SUMMONR" | cut -d' ' -f1)"
  echo "summonr_version $("$SUMMONR" --version | awk '{print $2}')"
  awk '$1=="summonr_commit"' "$root/tools/versions.txt"
  echo "base_packages $(python3 -c 'import level; print(*level.PACKAGES)')"
  echo "bench $bench"
  cat "$root/tools.lock"
  [ -f "$protocol" ] && echo "protocol $(sha256sum "$protocol" | cut -d' ' -f1)"
  echo "job $(sha256sum "$dir/job.yaml" | cut -d' ' -f1)"
} >> "$dir/run.txt"
harbor run -c "$dir/job.yaml" --job-name "$run" -o "$jobs" --plugin rss:Rss
[ -z "${BENCH_PREFLIGHT:-}" ] || exec python3 "$root/preflight.py" "$dir" "$root/logs/$run.jsonl" "$root/pairs/$pair.settings"
