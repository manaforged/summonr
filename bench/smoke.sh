#!/usr/bin/env bash
set -uo pipefail

root=$(cd "$(dirname "$0")" && pwd)
image=${BENCH_IMAGE:-alexgshaw/fix-git:20251031}
workdir=${BENCH_WORKDIR:-/app/personal-site}
proxy=${BENCH_PROXY:-http://172.17.0.1:8095}
log=${BENCH_PROXY_LOG:-$root/logs/glm.jsonl}
prompt=${BENCH_PROMPT:-Run git log --oneline -3 and reply with the subject of the newest commit.}
run=${BENCH_RUN:-smoke}
set -a; . "$root/glm.env"; set +a

printf '%s\n' "$prompt" > "$root/logs/prompt.txt"
for name in "${@:-summonr pi omp opencode hermes}"; do
  for harness in $name; do
    label=$harness.$run
    before=$(grep -c "\"label\": \"$label\"" "$log" 2>/dev/null || true)
    start=$(date +%s)
    docker run --rm \
      -e BENCH_BASE="$proxy/$label/v1" -e BENCH_MODEL=glm-5.3-flash -e ZAI_API_KEY ${BENCH_EFFORT:+-e BENCH_EFFORT} \
      -e BENCH_LOGS=/tmp/logs \
      -v "$root/tools":/opt/bench:ro -v "$root/logs/prompt.txt":/opt/prompt.txt:ro \
      -w "$workdir" "$image" timeout 300 bash /opt/bench/harness.sh "$harness" /opt/prompt.txt \
      > "$root/logs/$label.out" 2> "$root/logs/$label.err"
    code=$?
    after=$(grep -c "\"label\": \"$label\"" "$log" 2>/dev/null || true)
    echo "$harness exit=$code secs=$(( $(date +%s) - start )) requests=$(( after - before )) answer=$(grep -o 'off the job market woo' "$root/logs/$label.out" | head -1)"
  done
done
