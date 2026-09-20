#!/usr/bin/env bash
set -uo pipefail
root=$HOME/bench
lhtb=$root/lhtb
out=$lhtb/check/resume
log=$root/logs/smoke.jsonl
set -a; . "$root/deepseek.env"; set +a
mkdir -p "$out"
printf 'Remember the word PERIWINKLE. Do not use any tools. Reply only: ok\n' > "$out/p1.txt"
printf 'Do not use any tools or read any files. Which word did I ask you to remember? Reply with only that word.\n' > "$out/p2.txt"
one() {
  local harness=$1 label
  label=$(printf 'lhtb-resume.%s.%s' "$harness" "$(date +%s)" | sha256sum | cut -c1-16)
  echo "$harness $label" >> "$out/labels.txt"
  docker run --rm -e BENCH_BASE="http://172.17.0.1:8094/$label/v1" -e BENCH_MODEL=deepseek-flash -e DEEPSEEK_API_KEY \
    -e BENCH_LOGS=/logs/agent -e BENCH_EFFORT=high \
    -v "$root/tools":/opt/bench:ro -v "$lhtb/harness.sh":/opt/lhtb/harness.sh:ro -v "$out":/opt/p:ro \
    -w /app alexgshaw/fix-git:20251031 bash -c '
      timeout 300 bash /opt/lhtb/harness.sh '"$harness"' /opt/p/p1.txt new > /tmp/1.out 2> /tmp/1.err; a=$?
      timeout 300 bash /opt/lhtb/harness.sh '"$harness"' /opt/p/p2.txt resume > /tmp/2.out 2> /tmp/2.err; b=$?
      echo "exit1=$a exit2=$b"; echo "--- phase2 stdout tail"; tail -c 1500 /tmp/2.out; echo "--- phase2 stderr tail"; tail -c 600 /tmp/2.err' > "$out/$harness.txt" 2>&1
  python3 - "$log" "$label" "$harness" "$out/$harness.txt" <<'PY'
import json, sys
log, label, harness, path = sys.argv[1:]
reqs = [json.loads(l) for l in open(log) if f'"{label}"' in l]
users = [r["request"]["roles"].get("user", {}).get("count", 0) for r in reqs]
text = open(path).read()
tail = text.split("--- phase2 stdout tail", 1)[-1]
print(f"{harness} label={label} requests={len(reqs)} max_user_msgs={max(users or [0])} user_counts={users} {text.splitlines()[0]} phase2_says_word={'PERIWINKLE' in tail.upper()}")
PY
}
for h in "${@:-summonr pi omp opencode2 hermes goose}"; do for x in $h; do one "$x" & done; done
wait
