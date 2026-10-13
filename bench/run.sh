#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")" && pwd)
run=${1:?usage: run.sh RUN_ID TASKS_FILE [HARNESS...]}
tasks=${2:?usage: run.sh RUN_ID TASKS_FILE [HARNESS...]}
shift 2
harnesses=("${@:-summonr}")
read -ra harnesses <<< "${harnesses[*]}"
seed=${BENCH_SEED:-20260916}
effort=${BENCH_EFFORT:-high}
repeats=${BENCH_REPEATS:-5}
vendor=${BENCH_VENDOR:-deepseek}
tmult=${BENCH_TIMEOUT_MULT:-1.0}
parallel=${BENCH_PARALLEL:-1}
dataset=${BENCH_DATASET:-$root/datasets/tb2/terminal-bench-2}
export PATH=$HOME/.local/bin:$PATH PYTHONPATH=$root
routes=() drops=() paths=() side=""
case $vendor in
  zai)
    envfile=glm.env keyenv=ZAI_API_KEY port=8095 model=${BENCH_MODEL:-glm-5.3-flash}
    upstream=https://api.z.ai/api/coding/paas/v4 anthropic=https://api.z.ai/api/anthropic
    routes=(OPENCODE_GO_API_KEY=https://opencode.ai/zen/go/v1) drops=(opencode.ai=thinking,tool_stream) ;;
  openai)
    envfile=openai.env keyenv=OPENAI_API_KEY port=8096 model=${BENCH_MODEL:-gpt-6.1-sol}
    upstream=https://api.openai.com/v1 anthropic="" ;;
  claude)
    envfile="" keyenv="" port=8091 model=${BENCH_MODEL:-claude-opus-5-5}
    upstream=https://api.anthropic.com/v1 anthropic="" ;;
  chatgpt)
    envfile="" keyenv="" port=8092 model=${BENCH_MODEL:-gpt-6.1-sol}
    upstream=https://api.openai.com/v1 anthropic="" paths=(chatgpt=https://chatgpt.com/backend-api/codex) ;;
  supergrok)
    envfile="" keyenv="" port=8093 model=${BENCH_MODEL:-grok-4.7}
    upstream=https://api.x.ai/v1 anthropic="" paths=(grokcli=https://cli-chat-proxy.grok.com) side="grok-4.6-build" ;;
  *)
    envfile=deepseek.env keyenv=DEEPSEEK_API_KEY port=8094 model=${BENCH_MODEL:-deepseek-flash}
    upstream=https://api.deepseek.com anthropic=https://api.deepseek.com/anthropic ;;
esac
if [ -n "$envfile" ]; then
  [ -f "$root/$envfile" ] || { echo "missing $root/$envfile" >&2; exit 1; }
else
  logins=${BENCH_LOGINS:-$HOME}
  [ -d "$logins/.summonr" ] || { echo "missing $logins/.summonr: plan logins for $vendor" >&2; exit 1; }
fi
export BENCH_PROVIDER=${BENCH_PROVIDER:-$vendor} BENCH_MODEL=$model
export BENCH_PROXY=${BENCH_PROXY:-http://host.docker.internal:$port}

mkdir -p "$root/logs"
pkill -f "proxy.py .*--port $port " || true
for _ in $(seq 50); do (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null || break; sleep 0.2; done
keys=()
if [ -n "$envfile" ]; then
  for name in $(grep -oE "^${keyenv}(_[0-9]+)?=" "$root/$envfile" | tr -d =); do keys+=(--key-env "$name"); done
  for route in "${routes[@]}"; do
    grep -q "^${route%%=*}=" "$root/$envfile" && keys+=(--key-env "$route")
  done
fi
dropargs=()
for drop in "${drops[@]}"; do dropargs+=(--drop "$drop"); done
[ -n "$anthropic" ] && paths+=("anthropic=$anthropic")
for path in "${paths[@]}"; do dropargs+=(--route "$path"); done
(set -a; [ -z "$envfile" ] || . "$root/$envfile"; set +a
  exec setsid nohup python3 "$root/proxy.py" --bind 0.0.0.0 --port "$port" "${keys[@]}" "${dropargs[@]}" --log "$root/logs/$run.jsonl" ${BENCH_DUMP:+--dump "$root/logs/$run.bodies"} \
    --upstream "$upstream" >> "$root/logs/proxy-$run.out" 2>&1 < /dev/null) &
for _ in $(seq 50); do (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null && break; sleep 0.2; done
(exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null || { echo "proxy did not start on $port: see logs/proxy-$run.out" >&2; exit 1; }

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
  echo "side_models ${side:--}"
  echo "effort ${effort:-default}"
  echo "repeats $repeats"
  echo "timeout_mult $tmult"
  echo "order lockstep"
  echo "parallel $parallel"
  echo "keys $(( ${#keys[@]} / 2 ))"
  echo "routes $(printf "%s\n" "$upstream" "${keys[@]}" | grep -oE "https?://[^/ ]+" | sort -u | tr "\n" " ")"
  echo "harbor $(harbor --version)"
  echo "dataset_path $dataset"
  echo "dataset $got"
  echo "tasks $(sha256sum "$tasks" | cut -d' ' -f1)"
  echo "harness.sh $(sha256sum "$root/harness.sh" | cut -d' ' -f1)"
  echo "summonr $(awk '$1=="summonr"{print $2}' "$root/tools/versions.txt")"
  echo "summonr_commit $(awk '$1=="summonr_commit"{print $2}' "$root/tools/versions.txt")"
  awk '$1 ~ /^(summonr_version|pi|opencode2|hermes|omp|codex|claude|grok|bun|node)$/ {print $1, $2}' "$root/tools/versions.txt"
  echo "manifest $(sha256sum "$root/tools/manifest.sha256" | cut -d' ' -f1)"
} > "$root/jobs/$run/run.txt.new"
if [ -f "$root/jobs/$run/run.txt" ]; then
  while read -r key value; do
    case $key in
      seed|vendor|provider|model|effort|repeats|timeout_mult|harness.sh|dataset|tasks|summonr|summonr_commit|summonr_version|pi|opencode2|hermes|omp|codex|claude|grok|bun|node|manifest)
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
for rep in $(seq 1 "$repeats"); do
  grep -v '^[[:space:]]*$' "$tasks" | while read -r task; do
    for harness in "${harnesses[@]}"; do
      echo "$task $harness $rep"
    done
  done
done > "$root/jobs/$run/order.txt"

mounts='{"type":"bind","source":"'"$root"'/tools","target":"/opt/bench","read_only":true}'
if [ -z "$envfile" ]; then
  for tool in summonr codex grok claude; do
    if [ -d "$logins/.$tool" ]; then
      mounts+=',{"type":"bind","source":"'"$(readlink -f "$logins/.$tool")"'","target":"/opt/logins/'"$tool"'"}'
    fi
  done
fi
mounts="[$mounts]"
trial() {
  local task=$1 harness=$2 rep=$3
  local name=$harness.${task##*/}.r$rep
  local graded
  graded=$(find "$root/jobs/$run/$name" -mindepth 2 -maxdepth 2 -name result.json 2>/dev/null | head -1)
  if [ -n "$graded" ]; then
    python3 -c 'import json,sys; sys.exit(0 if json.load(open(sys.argv[1])).get("verifier_result") else 1)' "$graded" && return 0
    mv "$root/jobs/$run/$name" "$root/jobs/$run/.interrupted.$name.$(date +%s)"
  fi
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
pair() {
  local task=$1 rep=$2 harness
  for harness in $harnesses_list; do
    trial "$task" "$harness" "$rep" &
  done
  wait
}
export -f trial pair
harnesses_list="${harnesses[*]}"
export root run effort mounts dataset model BENCH_PROVIDER BENCH_PROXY tmult harnesses_list
for rep in $(seq 1 "$repeats"); do
  grep -v '^[[:space:]]*$' "$tasks" | xargs -P "$parallel" -I{} bash -c 'pair "$1" "$2"' _ {} "$rep"
done
docker images --digests --format '{{.Repository}}:{{.Tag}} {{.Digest}}' | grep -F -f <(sed 's/^/\//; s/$/:/' "$tasks") | sort > "$root/jobs/$run/images.txt"
