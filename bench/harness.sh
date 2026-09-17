#!/usr/bin/env bash
set -euo pipefail

name=${1:?usage: harness.sh NAME PROMPT_FILE}
prompt=$(cat "${2:?usage: harness.sh NAME PROMPT_FILE}")
: "${BENCH_BASE:?BENCH_BASE is the proxy URL ending in /v1}"
: "${BENCH_MODEL:?}"
: "${DEEPSEEK_API_KEY:?}"
tools=${BENCH_TOOLS:-/opt/bench}
home=$(mktemp -d /tmp/bench-home.XXXXXX)
logs=${BENCH_LOGS:-/logs/agent}
host=${BENCH_BASE%/v1}
[ -s /etc/ssl/certs/ca-certificates.crt ] || export SSL_CERT_FILE=$tools/cacert.pem
mkdir -p "$logs"

case "$name" in
summonr)
  mkdir -p "$logs/mage"
  printf '[modules]\nmcp = false\n' > "$logs/mage/config.toml"
  HOME=$home MAGE_HOME=$logs/mage MAGE_BASE_URL=$BENCH_BASE MAGE_MODEL=$BENCH_MODEL MAGE_API_KEY=$DEEPSEEK_API_KEY \
    "$tools/summonr/bin/summonr" -p "$prompt" --json ${BENCH_EFFORT:+--effort "$BENCH_EFFORT"}
  ;;
pi)
  mkdir -p "$home/pi"
  cat > "$home/pi/models.json" <<EOF
{"providers": {"deepseek": {"baseUrl": "$host", "models": [{"id":"$BENCH_MODEL","name":"DeepSeek V4 Flash","api":"openai-completions","baseUrl":"$host","provider":"deepseek","reasoning":true,"input":["text"],"cost":{"input":0.14,"output":0.28,"cacheRead":0.0028,"cacheWrite":0},"contextWindow":1000000,"maxTokens":384000,"compat":{"supportsStore":false,"supportsDeveloperRole":false,"maxTokensField":"max_tokens","requiresReasoningContentOnAssistantMessages":true,"thinkingFormat":"deepseek"},"thinkingLevelMap":{"minimal":null,"low":"low","medium":null,"high":"high","max":"max"}}]}}}
EOF
  HOME=$home PI_CODING_AGENT_DIR=$home/pi \
    "$tools/node/bin/node" "$tools/pi/lib/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js" \
    --print --mode json --provider deepseek --model "$BENCH_MODEL" ${BENCH_EFFORT:+--thinking "$BENCH_EFFORT"} "$prompt" </dev/null
  ;;
omp)
  mkdir -p "$home/.omp/agent"
  printf 'providers:\n  deepseek:\n    baseUrl: %s\n' "$host" > "$home/.omp/agent/models.yml"
  HOME=$home "$tools/bun/bun" "$tools/omp/lib/node_modules/@oh-my-pi/pi-coding-agent/dist/cli.js" \
    -p --mode json --no-session --yolo --model "deepseek/$BENCH_MODEL" ${BENCH_EFFORT:+--thinking "$BENCH_EFFORT"} "$prompt" </dev/null
  ;;
opencode)
  mkdir -p "$home/.config/opencode"
  cat > "$home/.config/opencode/opencode.json" <<EOF
{"\$schema": "https://opencode.ai/config.json", "provider": {"deepseek": {"options": {"baseURL": "$host"}}}}
EOF
  HOME=$home XDG_CONFIG_HOME=$home/.config XDG_DATA_HOME=$home/.local/share OPENCODE_DISABLE_MODELS_FETCH=1 \
    "$tools/opencode/bin/opencode" run --format json --model "deepseek/$BENCH_MODEL" ${BENCH_EFFORT:+--variant "$BENCH_EFFORT"} \
    --dangerously-skip-permissions -- "$prompt" </dev/null
  ;;
hermes)
  mkdir -p "$home/hermes"
  printf 'model:\n  provider: deepseek\n  default: %s\n' "$BENCH_MODEL" > "$home/hermes/config.yaml"
  HOME=$home HERMES_HOME=$home/hermes DEEPSEEK_BASE_URL=$BENCH_BASE \
    "$tools/hermes/.venv/bin/hermes" --yolo chat -q "$prompt" -Q ${BENCH_EFFORT:+--reasoning "$BENCH_EFFORT"} </dev/null
  ;;
goose)
  mkdir -p "$home/.config/goose/custom_providers"
  cat > "$home/.config/goose/custom_providers/custom_deepseek.json" <<EOF
{"name":"custom_deepseek","engine":"openai","display_name":"DeepSeek","description":"DeepSeek chat and reasoning models","api_key_env":"DEEPSEEK_API_KEY","base_url":"$host","models":[{"name":"deepseek-v4-flash","context_limit":128000,"input_token_cost":null,"output_token_cost":null,"currency":null,"supports_cache_control":null},{"name":"deepseek-v4-pro","context_limit":128000,"input_token_cost":null,"output_token_cost":null,"currency":null,"supports_cache_control":null},{"name":"deepseek-reasoner","context_limit":128000,"input_token_cost":null,"output_token_cost":null,"currency":null,"supports_cache_control":null}],"headers":null,"timeout_seconds":null,"preserves_thinking":true,"supports_streaming":true,"setup":{"category":"model","setup_method":"single_api_key","group":"additional","docs_url":"https://platform.deepseek.com/api_keys"}}
EOF
  HOME=$home GOOSE_DISABLE_KEYRING=true GOOSE_PROVIDER=custom_deepseek GOOSE_MODEL=$BENCH_MODEL \
    "$tools/goose/bin/goose" run --no-session --text "$prompt" </dev/null
  ;;
*)
  echo "unknown harness: $name" >&2
  exit 2
  ;;
esac
