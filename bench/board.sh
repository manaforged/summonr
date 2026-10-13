#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")" && pwd)
run=${1:?usage: board.sh RUN_ID PAIR}
pair=${2:?usage: board.sh RUN_ID PAIR}
template=$root/pairs/$pair.yaml
[ -f "$template" ] || { echo "no pair $pair in $root/pairs" >&2; exit 1; }
export PATH=$HOME/.local/bin:$PATH PYTHONPATH=$root
export LOGINS=${BENCH_LOGINS:-$HOME} SUMMONR=${BENCH_SUMMONR:-$root/tools/summonr/bin/summonr}
jobs=$root/jobs
dir=$jobs/$run

if [ -d "$dir" ]; then
  exec harbor job resume -p "$dir" --plugin rss:Rss
fi

fetch=$(mktemp -d)
trap 'rm -rf "$fetch"' EXIT
harbor download terminal-bench/terminal-bench-2-1 -o "$fetch" > /dev/null
got=$(cd "$fetch/terminal-bench-2-1" && find . -type f -print0 | sort -z | xargs -0 sha256sum | sha256sum | cut -d' ' -f1)
want=$(awk '$1=="terminal-bench-2-1"{print $2}' "$root/dataset.lock")
[ "$got" = "$want" ] || { echo "dataset lock: terminal-bench-2-1 wants $want, hub has $got" >&2; exit 1; }

mkdir -p "$dir"
python3 - "$template" "$root/tools.lock" "$dir/job.yaml" > "$dir/run.txt" <<'EOF'
import os, re, string, sys
pins = dict(line.split() for line in open(sys.argv[2]) if line.strip())
text = string.Template(open(sys.argv[1]).read()).substitute({**pins, **os.environ})
open(sys.argv[3], "w").write(text)
print("model", re.search(r"model_name: [^/\s]+/(\S+)", text)[1])
print("effort", re.search(r"reasoning_effort: (\w+)", text)[1])
print("repeats", re.search(r"n_attempts: (\d+)", text)[1])
print("timeout_mult", re.search(r"timeout_multiplier: ([\d.]+)", text)[1])
EOF
{
  echo "vendor $pair"
  echo "harbor $(harbor --version)"
  echo "dataset $got"
  echo "summonr $(sha256sum "$SUMMONR" | cut -d' ' -f1)"
  echo "summonr_version $("$SUMMONR" --version | awk '{print $2}')"
  awk '$1=="summonr_commit"' "$root/tools/versions.txt"
  echo "job $(sha256sum "$dir/job.yaml" | cut -d' ' -f1)"
} >> "$dir/run.txt"
harbor run -c "$dir/job.yaml" --job-name "$run" -o "$jobs" --plugin rss:Rss
