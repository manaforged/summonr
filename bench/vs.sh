#!/usr/bin/env bash
# summonr vs pi on the same fixed prompts, tokens counted at the proxy.
#
#   vs.sh TASKS_FILE [SUMMONR_BIN]
#
# TASKS_FILE: one prompt per line (# comments and blanks skipped).
# Each prompt runs in its own detached worktree of this repo at HEAD.
# AB_CONCURRENT (default 8) sets the pool width, AB_REPEATS the trials.
# Both arms hit bench/proxy.py on 127.0.0.1:8095 with a URL label, so
# tokens, cache hit, latency, and tool-call names come from the same
# accounting for both CLIs. DEEPSEEK_API_KEY feeds the pi arm; when unset
# it is read from the deepseek api_key in config.toml.bak-preclobber-*.
set -euo pipefail

tasks=${1:?usage: vs.sh TASKS_FILE [SUMMONR_BIN]}
summonr=${2:-$(cd "$(dirname "$0")/.." && pwd)/target/release/summonr}
[ -x "$summonr" ] || summonr=${summonr/release/debug}
tasks=$(cd "$(dirname "$tasks")" && pwd)/$(basename "$tasks")
summonr=$(cd "$(dirname "$summonr")" && pwd)/$(basename "$summonr")
repo=$(cd "$(dirname "$0")/.." && pwd)
repeats=${AB_REPEATS:-1}
concurrent=${AB_CONCURRENT:-8}
port=${VS_PORT:-8095}
model=${BENCH_MODEL:-deepseek-v4-flash}
out=$(mktemp -d)/vs
mkdir -p "$out/works"
: > "$out/proxy.jsonl"

if [ -z "${DEEPSEEK_API_KEY:-}" ]; then
    bak=$(ls -t "$HOME"/.summonr/config.toml.bak-preclobber-* 2>/dev/null | head -1)
    DEEPSEEK_API_KEY=$(awk '/\[endpoint\.deepseek\]/,/^\[endpoint\.deepseek-v/{if (/^api_key/) {gsub(/[" ]/, "", $3); print $3; exit}}' "$bak")
fi
[ -n "$DEEPSEEK_API_KEY" ] || { echo "vs.sh: no DEEPSEEK_API_KEY and none found in config backups" >&2; exit 1; }

python3 "$repo/bench/proxy.py" --bind 127.0.0.1 --port "$port" \
    --log "$out/proxy.jsonl" --upstream https://api.deepseek.com/v1 &
proxy_pid=$!
disown
trap 'kill $proxy_pid 2>/dev/null || true' EXIT
sleep 0.5

run_pool() {
    local label=$1 home=$2
    local task=0
    while IFS= read -r prompt; do
        case "$prompt" in ''|\#*) continue ;; esac
        task=$((task + 1))
        for repeat in $(seq "$repeats"); do
            while [ "$(jobs -pr | wc -l)" -ge "$concurrent" ]; do sleep 0.3; done
            run_one "$label" "$home" "$task" "$repeat" "$prompt" &
        done
    done < "$tasks"
    wait
}

run_one() {
    local label=$1 home=$2 task=$3 repeat=$4 prompt=$5
    local work="$out/works/$label-$task-$repeat"
    git -C "$repo" worktree add --detach "$work" HEAD >/dev/null 2>&1 || return
    if [ "$label" = summonr ]; then
        (cd "$work" && SUMMONR_HOME="$home" \
            /usr/bin/time -l "$summonr" -e deepseek -p "$prompt") \
            >"$out/$label-$task-$repeat.stdout" 2>"$out/$label-$task-$repeat.time" || true
    else
        (cd "$work" && HOME="$home" PI_CODING_AGENT_DIR="$home/pi" \
            DEEPSEEK_API_KEY="$DEEPSEEK_API_KEY" \
            /usr/bin/time -l pi --print --mode json --provider deepseek --model "$model" \
            --session-dir "$home/pi-sessions" "$prompt" </dev/null) \
            >"$out/$label-$task-$repeat.stdout" 2>"$out/$label-$task-$repeat.time" || true
    fi
    git -C "$repo" worktree remove --force "$work" >/dev/null 2>&1 || true
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
EOF

pi_home="$out/pi-home"
mkdir -p "$pi_home/pi"
cat > "$pi_home/pi/models.json" <<EOF
{"providers": {"deepseek": {"baseUrl": "http://127.0.0.1:$port/pi", "models": [{"id":"$model","name":"$model","api":"openai-completions","baseUrl":"http://127.0.0.1:$port/pi","provider":"deepseek","reasoning":true,"input":["text"],"cost":{"input":0.3,"output":1.2,"cacheRead":0.006,"cacheWrite":0},"contextWindow":1000000,"maxTokens":384000,"compat":{"supportsStore":false,"supportsDeveloperRole":false,"maxTokensField":"max_tokens","requiresReasoningContentOnAssistantMessages":true,"thinkingFormat":"deepseek"},"thinkingLevelMap":{"minimal":null,"low":"low","medium":null,"high":"high","max":"max"}}]}}}
EOF

run_pool summonr "$summonr_home"
run_pool pi "$pi_home"

SUMMONR_HOME="$summonr_home" "$summonr" profile --all > "$out/summonr.profile" || true
python3 - "$out" <<'PY'
import json, sys, collections
out = sys.argv[1]
rows = collections.defaultdict(lambda: {"requests": 0, "in": 0, "out": 0,
    "cached": 0, "latency": 0.0, "tools": collections.Counter()})
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
import glob, re, statistics
rss = collections.defaultdict(list)
for f in glob.glob(f"{out}/*.time"):
    label = f.rsplit("/", 1)[-1].split("-")[0]
    text = open(f, errors="replace").read()
    m = re.findall(r"(\d+)\s+maximum resident set size", text)
    if m:
        rss[label].append(int(m[-1]))
for label, row in sorted(rows.items()):
    calls = " · ".join(f"{name} {count}" for name, count in row["tools"].most_common())
    cached_pct = 100 * row["cached"] / row["in"] if row["in"] else 0
    print(f"== {label} ==")
    print(f"requests {row['requests']} · in {row['in']:,} (cached {row['cached']:,}, {cached_pct:.0f}%) · out {row['out']:,} · latency {row['latency']:.0f}s")
    print(f"calls    {calls}")
    if rss.get(label):
        print(f"rss      peak median {statistics.median(rss[label])/1048576:.0f} MB · max {max(rss[label])/1048576:.0f} MB")
    print()
PY

echo "vs bench $out · endpoint deepseek · model $model · repeats $repeats · tasks $tasks"
cat "$out/summonr.profile"
