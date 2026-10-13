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
logins=${BENCH_LOGINS:-/opt/logins}

share() {
  local dir=$1 dest=$2
  shift 2
  mkdir -p "$dest"
  for file in "$@"; do
    ln -sfn "$logins/$dir/$file" "$dest/$file"
  done
}

restore() {
  local dir
  for dir in "$logins"/*/; do
    find "$dir" -maxdepth 1 -newer "$home" ! -user "$(stat -c %u "$dir")" -exec chown --reference="$dir" {} + 2>/dev/null || true
  done
}

claude_seat() {
  [ -d "$logins/claude" ] && share claude "$home/.claude" .credentials.json
  export ANTHROPIC_BASE_URL=${BENCH_BASE%/v1} PATH="$tools/claude/bin:$PATH"
}

agent() {
case "$name" in
summonr | summonr-*)
  mkdir -p "$logs/$name"
  if [ -n "${BENCH_KEEP_REASONING:-}" ]; then
    export SUMMONR_KEEP_REASONING=1
  fi
  case "$provider" in
  openai | chatgpt) pick=openai ;;
  supergrok) pick=grok ;;
  claude) pick=claude ;;
  *) pick="" ;;
  esac
  if [ "$pick" = claude ]; then
    claude_seat
    printf '[endpoint.claude]\nmodel = "%s"\n' "$BENCH_MODEL" > "$logs/$name/config.toml"
  elif [ -n "$pick" ]; then
    printf '[endpoint.%s]\nbase_url = "%s"\nmodel = "%s"\n' "$pick" "$BENCH_BASE" "$BENCH_MODEL" > "$logs/$name/config.toml"
  fi
  if [ -d "$logins/summonr" ] && [ "$pick" != claude ]; then
    mkdir -p "$logins/summonr/locks"
    share summonr "$logs/$name" credentials.toml credentials.wrap credentials.lock locks
    export SUMMONR_SECRETS=file
  elif [ "$pick" = openai ]; then
    printf '%s\n' "$key" | HOME=$home SUMMONR_HOME=$logs/$name "$tools/$name/bin/summonr" login openai --key >/dev/null
  fi
  HOME=$home SUMMONR_HOME=$logs/$name SUMMONR_BASE_URL=$BENCH_BASE SUMMONR_MODEL=$BENCH_MODEL SUMMONR_API_KEY=$key \
    SUMMONR_MAX_TOKENS=$max_tokens \
    "$tools/$name/bin/summonr" ${pick:+-e "$pick"} --json ${BENCH_EFFORT:+--effort "$BENCH_EFFORT"} -p -- "$prompt"
  ;;
codex)
  mkdir -p "$logs/codex"
  base=$BENCH_BASE auth="env_key=\"BENCH_API_KEY\""
  if [ -d "$logins/codex" ]; then
    share codex "$logs/codex" auth.json
    base=${BENCH_BASE%/v1}/chatgpt auth="requires_openai_auth=true"
  fi
  HOME=$home CODEX_HOME=$logs/codex BENCH_API_KEY=$key \
    "$tools/codex/bin/codex" exec --json --skip-git-repo-check --dangerously-bypass-approvals-and-sandbox -m "$BENCH_MODEL" \
    ${BENCH_EFFORT:+-c "model_reasoning_effort=$BENCH_EFFORT"} -c model_provider=bench \
    -c "model_providers.bench={name=\"bench\",base_url=\"$base\",$auth,wire_api=\"responses\",supports_websockets=false,supports_standalone_web_search=true}" \
    -- "$prompt" </dev/null
  ;;
claude)
  claude_seat
  HOME=$home ANTHROPIC_MODEL=$BENCH_MODEL ANTHROPIC_DEFAULT_OPUS_MODEL=$BENCH_MODEL ANTHROPIC_DEFAULT_SONNET_MODEL=$BENCH_MODEL \
    ANTHROPIC_DEFAULT_HAIKU_MODEL=$BENCH_MODEL CLAUDE_CODE_SUBAGENT_MODEL=$BENCH_MODEL \
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 DISABLE_AUTOUPDATER=1 IS_SANDBOX=1 \
    "$tools/claude/bin/claude" -p --dangerously-skip-permissions --output-format json --model "$BENCH_MODEL" \
    ${BENCH_EFFORT:+--effort "$BENCH_EFFORT"} -- "$prompt" </dev/null
  ;;
grok)
  HOME=$home GROK_AUTH_PATH=$logins/grok/auth.json GROK_DISABLE_AUTOUPDATER=1 GROK_CLI_CHAT_PROXY_BASE_URL=${BENCH_BASE%/v1}/grokcli/v1 \
    "$tools/grok/bin/grok" --single "$prompt" --output-format streaming-json -m "$BENCH_MODEL" --always-approve \
    --debug-file "$logs/grok-debug.log" ${BENCH_EFFORT:+--reasoning-effort "$BENCH_EFFORT"} </dev/null
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
opencode2)
  case "$provider" in
  zai*) ocprovider=zai-coding-plan ;;
  *) ocprovider=$provider ;;
  esac
  mkdir -p "$home/.config/opencode"
  printf '{"$schema": "https://opencode.ai/config.json", "provider": {"%s": {"options": {"baseURL": "%s", "apiKey": "{env:BENCH_API_KEY}"}}}}\n' \
    "$ocprovider" "$BENCH_BASE" > "$home/.config/opencode/opencode.json"
  HOME=$home XDG_CONFIG_HOME=$home/.config XDG_DATA_HOME=$home/.local/share XDG_STATE_HOME=$home/.local/state XDG_CACHE_HOME=$home/.cache \
    BENCH_API_KEY=$key \
    "$tools/opencode/bin/opencode2" run --standalone --auto --format json \
    --model "$ocprovider/$BENCH_MODEL${BENCH_EFFORT:+#$BENCH_EFFORT}" -- "$prompt" </dev/null
  ;;
omp)
  mkdir -p "$home/.omp/agent"
  printf 'providers:\n  %s:\n    baseUrl: %s\n    modelOverrides:\n      %s:\n        maxTokens: %s\n' \
    "$provider" "$BENCH_BASE" "$BENCH_MODEL" "$max_tokens" > "$home/.omp/agent/models.yml"
  HOME=$home ZAI_API_KEY=$key DEEPSEEK_API_KEY=$key \
    "$tools/bun/bun" "$tools/omp/lib/node_modules/@oh-my-pi/pi-coding-agent/dist/cli.js" \
    -p --mode json --no-session --yolo --model "$provider/$BENCH_MODEL" ${BENCH_EFFORT:+--thinking "$BENCH_EFFORT"} "$prompt" </dev/null
  ;;
hermes)
  case "$provider" in
  zai*) hprovider=zai baseenv=GLM_BASE_URL ;;
  *) hprovider=$provider baseenv=DEEPSEEK_BASE_URL ;;
  esac
  mkdir -p "$home/hermes"
  printf 'model:\n  provider: %s\n  default: %s\n' "$hprovider" "$BENCH_MODEL" > "$home/hermes/config.yaml"
  env HOME="$home" HERMES_HOME="$home/hermes" "$baseenv=$BENCH_BASE" \
    "$tools/hermes/.venv/bin/hermes" --yolo chat -q "$prompt" -Q ${BENCH_EFFORT:+--reasoning "$BENCH_EFFORT"} </dev/null
  ;;
*)
  echo "unknown harness: $name" >&2
  exit 2
  ;;
esac
}

memory() {
  awk -v root="$1" '
    function keep() { if (pid != "") { parent[pid] = ppid; rss[pid] = kb; shell[pid] = nm ~ /^(ba|da|z|k|a)?sh$/ } }
    FNR == 1 { keep(); pid = ""; ppid = ""; kb = 0; nm = "" }
    $1 == "Name:" { nm = $2 }
    $1 == "Pid:" { pid = $2 }
    $1 == "PPid:" { ppid = $2 }
    $1 == "VmRSS:" { kb = $2 }
    END {
      keep()
      for (p in parent) {
        q = p; n = 0; viashell = 0
        while (q != root && q in parent && n++ < 64) { if (shell[q]) viashell = 1; q = parent[q] }
        if (q != root || p == root) continue
        tree += rss[p]
        if (parent[p] == root) own += rss[p]
        if (!viashell) helpers += rss[p]
      }
      print own + 0, tree + 0, helpers + 0
    }' /proc/[0-9]*/status 2>/dev/null
}

agent &
pid=$!
peak_own=0
peak_tree=0
peak_harness=0
sum_own=0
sum_tree=0
sum_harness=0
samples=0
while kill -0 "$pid" 2>/dev/null; do
  read -r own tree harness < <(memory "$pid") || true
  [ "${own:-0}" -gt "$peak_own" ] && peak_own=$own
  [ "${tree:-0}" -gt "$peak_tree" ] && peak_tree=$tree
  [ "${harness:-0}" -gt "$peak_harness" ] && peak_harness=$harness
  if [ "${own:-0}" -gt 0 ]; then
    sum_own=$((sum_own + own)) sum_tree=$((sum_tree + tree)) sum_harness=$((sum_harness + ${harness:-0})) samples=$((samples + 1))
    printf 'agent_kb %s\ntree_kb %s\nharness_kb %s\nagent_avg_kb %s\ntree_avg_kb %s\nharness_avg_kb %s\n' "$peak_own" "$peak_tree" "$peak_harness" \
      "$((sum_own / samples))" "$((sum_tree / samples))" "$((sum_harness / samples))" > "$logs/memory.txt"
  fi
  sleep 0.5
done
status=0
wait "$pid" || status=$?
find "$logs" \( -name credentials.toml -o -name credentials.wrap -o -name credentials.lock -o -name locks -o -name auth.json \) -type l -delete
restore
exit "$status"
