#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")" && pwd)
run=${1:?usage: run.sh RUN_ID TASKS_FILE [HARNESS...]}
tasks=${2:?usage: run.sh RUN_ID TASKS_FILE [HARNESS...]}
shift 2
harnesses=("${@:-summonr}")
read -ra harnesses <<< "${harnesses[*]}"
seed=${BENCH_SEED:-20260916}
effort=${BENCH_EFFORT:-}
concurrent=${BENCH_CONCURRENT:-2}
repeats=${BENCH_REPEATS:-5}
vendor=${BENCH_VENDOR:-deepseek}
tmult=${BENCH_TIMEOUT_MULT:-1.0}
dataset=${BENCH_DATASET:-$root/datasets/tb2/terminal-bench-2}
export PATH=$HOME/.local/bin:$PATH PYTHONPATH=$root
if [ "$vendor" = zai ]; then
  [ -f "$root/glm.env" ] || { echo "missing $root/glm.env" >&2; exit 1; }
  set -a; . "$root/glm.env"; set +a
  : "${ZAI_API_KEY:?}"
  export BENCH_API_KEY=$ZAI_API_KEY
  export BENCH_PROVIDER=${BENCH_PROVIDER:-zai}
  model=${BENCH_MODEL:-glm-5.3-flash}
else
  [ -f "$root/deepseek.env" ] || { echo "missing $root/deepseek.env" >&2; exit 1; }
  set -a; . "$root/deepseek.env"; set +a
  : "${DEEPSEEK_API_KEY:?}"
  export BENCH_API_KEY=$DEEPSEEK_API_KEY
  export BENCH_PROVIDER=${BENCH_PROVIDER:-deepseek}
  model=${BENCH_MODEL:-deepseek-flash}
fi
export DEEPSEEK_API_KEY=$BENCH_API_KEY
export BENCH_MODEL=$model

case $vendor in
  zai) port=8095 ;;
  *) port=8096 ;;
esac
export BENCH_PROXY=${BENCH_PROXY:-http://host.docker.internal:$port}

mkdir -p "$root/jobs/$run"
if [ -d "$dataset" ]; then
  got=$(cd "$dataset" && find . -type f -print0 | sort -z | xargs -0 sha256sum | sha256sum | cut -d' ' -f1)
else
  got="hub:$dataset"
fi
if [ -f "$root/dataset.lock" ]; then
  want=$(awk -v name="$(basename "$dataset")" '$1==name{print $2}' "$root/dataset.lock")
  if [ -z "$want" ]; then
    echo "dataset lock: no entry for $(basename "$dataset") in dataset.lock" >&2
    exit 1
  fi
  if [ "$want" != "$got" ]; then
    echo "dataset lock: $(basename "$dataset") wants $want, got $got" >&2
    exit 1
  fi
fi
{
  echo "seed $seed"
  echo "vendor $vendor"
  echo "provider $BENCH_PROVIDER"
  echo "model $model"
  echo "effort ${effort:-default}"
  echo "repeats $repeats"
  echo "timeout_mult $tmult"
  echo "concurrent $concurrent"
  echo "harbor $(harbor --version)"
  echo "dataset_path $dataset"
  echo "dataset $got"
  echo "tasks $(sha256sum "$tasks" | cut -d' ' -f1)"
  echo "harness.sh $(sha256sum "$root/harness.sh" | cut -d' ' -f1)"
  echo "summonr $(awk '$1=="summonr"{print $2}' "$root/tools/versions.txt")"
  echo "summonr_commit $(awk '$1=="summonr_commit"{print $2}' "$root/tools/versions.txt")"
  echo "manifest $(sha256sum "$root/tools/manifest.sha256" | cut -d' ' -f1)"
} > "$root/jobs/$run/run.txt.new"
if [ -f "$root/jobs/$run/run.txt" ]; then
  while read -r key value; do
    case $key in
      seed|vendor|provider|model|effort|repeats|timeout_mult|harness.sh|dataset|tasks|summonr|summonr_commit|manifest)
        old=$(awk -v k="$key" '$1==k{print $2}' "$root/jobs/$run/run.txt")
        if [ -n "$old" ] && [ "$old" != "$value" ]; then
          rm -f "$root/jobs/$run/run.txt.new"
          echo "resume mismatch $key: recorded $old, now $value" >&2
          exit 1
        fi ;;
    esac
  done < "$root/jobs/$run/run.txt.new"
  rm "$root/jobs/$run/run.txt.new"
else
  mv "$root/jobs/$run/run.txt.new" "$root/jobs/$run/run.txt"
fi
cp "$root/tools/versions.txt" "$root/tools/manifest.sha256" "$root/harness.sh" "$tasks" "$root/jobs/$run/"
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
  local name=$harness.${task##*/}.r$rep
  [ -f "$root/jobs/$run/$name/result.json" ] && return 0
  local -a ds
  if [[ "$dataset" == */* && "$dataset" != /* ]]; then
    ds=(-d "$dataset")
  else
    ds=(-p "$dataset")
  fi
  harbor run "${ds[@]}" -i "$task" --agent agent:Bench -o "$root/jobs/$run" --job-name "$name" \
    --mounts-json "$mounts" --ek extra_hosts=host.docker.internal:host-gateway \
    --agent-timeout-multiplier "$tmult" \
    --ae BENCH_HARNESS="$harness" --ae BENCH_RUN="$run" \
    --ae BENCH_PROXY="$BENCH_PROXY" --ae BENCH_MODEL="$model" --ae BENCH_PROVIDER="$BENCH_PROVIDER" ${effort:+--ae BENCH_EFFORT="$effort"} -n 1 \
    > "$root/jobs/$run/$name.log" 2>&1 || echo "harbor failed: $name" >> "$root/jobs/$run/failures.txt"
  echo "$(date -u +%FT%TZ) done $name"
}
export -f trial
export root run effort mounts dataset model BENCH_PROVIDER BENCH_PROXY tmult
xargs -P "$concurrent" -L 1 bash -c 'trial "$0" "$1" "$2"' < "$root/jobs/$run/order.txt"
docker images --digests --format '{{.Repository}}:{{.Tag}} {{.Digest}}' | grep -F -f <(sed 's/^/\//; s/$/:/' "$tasks") | sort > "$root/jobs/$run/images.txt"
