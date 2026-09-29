#!/usr/bin/env bash
set -euo pipefail

name=${1:?usage: harness.sh NAME PROMPT_FILE}
prompt=$(cat "${2:?usage: harness.sh NAME PROMPT_FILE}")
: "${BENCH_BASE:?BENCH_BASE is the proxy URL ending in /v1}"
: "${BENCH_MODEL:?}"
key=${BENCH_API_KEY:-${DEEPSEEK_API_KEY:?}}
provider=${BENCH_PROVIDER:-deepseek}
case "$provider" in
zai*) max_tokens=${BENCH_MAX_TOKENS:-131072} ;;
*) max_tokens=${BENCH_MAX_TOKENS:-384000} ;;
esac
tools=${BENCH_TOOLS:-/opt/bench}
home=$(mktemp -d /tmp/bench-home.XXXXXX)
logs=${BENCH_LOGS:-/logs/agent}
host=${BENCH_BASE%/v1}
if [[ $BENCH_BASE == *host.docker.internal* ]]; then
  gwh=$(awk '$2=="00000000"{print $3; exit}' /proc/net/route)
  gw=$(printf "%d.%d.%d.%d" 0x${gwh:6:2} 0x${gwh:4:2} 0x${gwh:2:2} 0x${gwh:0:2})
  BENCH_BASE=${BENCH_BASE//host.docker.internal/$gw}
  host=${BENCH_BASE%/v1}
fi
export DEEPSEEK_API_KEY=$key ZAI_API_KEY=$key OCGO_API_KEY=$key OPENCODE_API_KEY=$key
export PATH="$tools/node/bin:$PATH"
unset ANTHROPIC_API_KEY OPENAI_API_KEY XAI_API_KEY CLAUDE_CODE_OAUTH_TOKEN CHATGPT_API_KEY || true
if [ ! -s /etc/ssl/certs/ca-certificates.crt ]; then
  mkdir -p /etc/ssl/certs 2>/dev/null || true
  cp "$tools/cacert.pem" /etc/ssl/certs/ca-certificates.crt 2>/dev/null || export SSL_CERT_FILE=$tools/cacert.pem
fi
mkdir -p "$logs"

agent() {
case "$name" in
summonr)
  mkdir -p "$logs/summonr"
  if [ -n "${BENCH_KEEP_REASONING:-}" ]; then
    export SUMMONR_KEEP_REASONING=1
  fi
  HOME=$home SUMMONR_HOME=$logs/summonr SUMMONR_BASE_URL=$BENCH_BASE SUMMONR_MODEL=$BENCH_MODEL SUMMONR_API_KEY=$key \
    SUMMONR_MAX_TOKENS=$max_tokens \
    "$tools/summonr/bin/summonr" --json ${BENCH_EFFORT:+--effort "$BENCH_EFFORT"} ${BENCH_MAX_SECS:+--max-secs "$BENCH_MAX_SECS"} -p -- "$prompt"
  ;;
pi)
  mkdir -p "$home/pi"
  case "$provider" in
  deepseek)
    piprovider=$provider
    cat > "$home/pi/models.json" <<EOF
{"providers": {"$provider": {"baseUrl": "$host", "apiKey": "\$BENCH_API_KEY", "models": [{"id":"$BENCH_MODEL","name":"$BENCH_MODEL","api":"openai-completions","baseUrl":"$host","provider":"$provider","reasoning":true,"input":["text"],"cost":{"input":0.3,"output":1.2,"cacheRead":0.006,"cacheWrite":0},"contextWindow":1000000,"maxTokens":$max_tokens,"compat":{"supportsStore":false,"supportsDeveloperRole":false,"maxTokensField":"max_tokens","requiresReasoningContentOnAssistantMessages":true,"thinkingFormat":"deepseek"},"thinkingLevelMap":{"minimal":null,"low":"low","medium":null,"high":"high","max":"max"}}]}}}
EOF
    ;;
  *)
    piprovider=$provider
    [ "$provider" = ocgo ] && piprovider=opencode-go
    cat > "$home/pi/models.json" <<EOF
{"providers": {"$piprovider": {"baseUrl": "$BENCH_BASE", "apiKey": "\$BENCH_API_KEY", "modelOverrides": {"$BENCH_MODEL": {"maxTokens": $max_tokens}}}}}
EOF
    ;;
  esac
  HOME=$home PI_CODING_AGENT_DIR=$home/pi DEEPSEEK_API_KEY=$key BENCH_API_KEY=$key \
    "$tools/node/bin/node" "$tools/pi/lib/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js" \
    --print --mode json --provider "$piprovider" --model "$BENCH_MODEL" ${BENCH_EFFORT:+--thinking "$BENCH_EFFORT"} -- "$prompt" </dev/null
  ;;
*)
  echo "unknown harness: $name" >&2
  exit 2
  ;;
esac
}

memory() {
  awk -v root="$1" '
    function keep() { if (pid != "") { parent[pid] = ppid; rss[pid] = kb } }
    FNR == 1 { keep(); pid = ""; ppid = ""; kb = 0 }
    $1 == "Pid:" { pid = $2 }
    $1 == "PPid:" { ppid = $2 }
    $1 == "VmRSS:" { kb = $2 }
    END {
      keep()
      for (p in parent) {
        q = p; n = 0
        while (q != root && q in parent && n++ < 64) q = parent[q]
        if (q != root || p == root) continue
        tree += rss[p]
        if (parent[p] == root) own += rss[p]
      }
      print own + 0, tree + 0
    }' /proc/[0-9]*/status 2>/dev/null
}

agent &
pid=$!
peak_own=0
peak_tree=0
while kill -0 "$pid" 2>/dev/null; do
  read -r own tree < <(memory "$pid") || true
  [ "${own:-0}" -gt "$peak_own" ] && peak_own=$own
  [ "${tree:-0}" -gt "$peak_tree" ] && peak_tree=$tree
  printf 'agent_kb %s\ntree_kb %s\n' "$peak_own" "$peak_tree" > "$logs/memory.txt"
  sleep 0.5
done
wait "$pid"
