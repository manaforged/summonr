#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")" && pwd)
run=${1:?usage: run.sh RUN_ID TASKS_FILE [HARNESS...]}
tasks=${2:?usage: run.sh RUN_ID TASKS_FILE [HARNESS...]}
shift 2
harnesses=("${@:-summonr pi omp opencode hermes}")
read -ra harnesses <<< "${harnesses[*]}"
seed=${BENCH_SEED:-20260916}
effort=${BENCH_EFFORT:-}
concurrent=${BENCH_CONCURRENT:-2}
repeats=${BENCH_REPEATS:-1}
dataset=${BENCH_DATASET:-$root/datasets/tb2/terminal-bench-2}
export PATH=$HOME/.local/bin:$PATH PYTHONPATH=$root
set -a; . "$root/glm.env"; set +a

mkdir -p "$root/jobs/$run"
cp "$root/tools/versions.txt" "$root/tools/manifest.sha256" "$root/harness.sh" "$tasks" "$root/jobs/$run/"
{
  echo "seed $seed"
  echo "effort ${effort:-default}"
  echo "repeats $repeats"
  echo "concurrent $concurrent"
  echo "harbor $(harbor --version)"
  echo "dataset $(cd "$dataset" && find . -type f -print0 | sort -z | xargs -0 sha256sum | sha256sum | cut -d' ' -f1)"
  echo "harness.sh $(sha256sum "$root/harness.sh" | cut -d' ' -f1)"
} > "$root/jobs/$run/run.txt"
python3 - "$tasks" "$seed" "$repeats" "${harnesses[@]}" > "$root/jobs/$run/order.txt" <<'EOF'
import random, sys
tasks = [line.strip() for line in open(sys.argv[1]) if line.strip()]
pairs = [(task, harness, rep) for rep in range(1, int(sys.argv[3]) + 1) for task in tasks for harness in sys.argv[4:]]
random.Random(int(sys.argv[2])).shuffle(pairs)
for task, harness, rep in pairs:
    print(task, harness, rep)
EOF

mounts='[{"type":"bind","source":"'"$root"'/tools","target":"/opt/bench","read_only":true}]'
trial() {
  local task=$1 harness=$2 rep=$3
  local name=$harness.$task.r$rep
  [ -f "$root/jobs/$run/$name/result.json" ] && return 0
  harbor run -p "$dataset" -i "$task" --agent-import-path agent:Bench -o "$root/jobs/$run" --job-name "$name" \
    --mounts-json "$mounts" --ae BENCH_HARNESS="$harness" --ae BENCH_RUN="$run" \
    --ae BENCH_PROXY=http://172.17.0.1:8095 ${effort:+--ae BENCH_EFFORT="$effort"} -n 1 \
    > "$root/jobs/$run/$name.log" 2>&1 || echo "harbor failed: $name" >> "$root/jobs/$run/failures.txt"
  echo "$(date -u +%FT%TZ) done $name"
}
export -f trial
export root run effort mounts dataset
xargs -P "$concurrent" -L 1 bash -c 'trial "$0" "$1" "$2"' < "$root/jobs/$run/order.txt"
docker images --digests --format '{{.Repository}}:{{.Tag}} {{.Digest}}' | grep -F -f <(sed 's/^/\//; s/$/:/' "$tasks") | sort > "$root/jobs/$run/images.txt"
