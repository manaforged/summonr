#!/usr/bin/env bash
set -euo pipefail

name=${1:?usage: harness.sh NAME PROMPT_FILE}
prompt=$(cat "${2:?usage: harness.sh NAME PROMPT_FILE}")
: "${BENCH_BASE:?BENCH_BASE is the proxy URL ending in /v1}"
: "${BENCH_MODEL:?}"
key=${BENCH_API_KEY:-${DEEPSEEK_API_KEY:?}}
provider=${BENCH_PROVIDER:-deepseek}
tools=${BENCH_TOOLS:-/opt/bench}
home=$(mktemp -d /tmp/bench-home.XXXXXX)
logs=${BENCH_LOGS:-/logs/agent}
host=${BENCH_BASE%/v1}
if [[ $BENCH_BASE == *host.docker.internal* ]]; then
  gw=$(python3 -c "import pathlib; g=pathlib.Path('/proc/net/route').read_text().splitlines()[1].split()[2]; b=bytes.fromhex(g); print('.'.join(str(x) for x in b[::-1]))")
  BENCH_BASE=${BENCH_BASE//host.docker.internal/$gw}
  host=${BENCH_BASE%/v1}
fi
export DEEPSEEK_API_KEY=$key
export PATH="$tools/node/bin:$PATH"
unset ANTHROPIC_API_KEY OPENAI_API_KEY XAI_API_KEY CLAUDE_CODE_OAUTH_TOKEN CHATGPT_API_KEY || true
[ -s /etc/ssl/certs/ca-certificates.crt ] || export SSL_CERT_FILE=$tools/cacert.pem
mkdir -p "$logs"

case "$name" in
summonr)
  mkdir -p "$logs/summonr"
  HOME=$home SUMMONR_HOME=$logs/summonr SUMMONR_BASE_URL=$BENCH_BASE SUMMONR_MODEL=$BENCH_MODEL SUMMONR_API_KEY=$key \
    "$tools/summonr/bin/summonr" -p "$prompt" --json ${BENCH_EFFORT:+--effort "$BENCH_EFFORT"}
  ;;
pi)
  mkdir -p "$home/pi"
  cat > "$home/pi/models.json" <<EOF
{"providers": {"$provider": {"baseUrl": "$host", "models": [{"id":"$BENCH_MODEL","name":"$BENCH_MODEL","api":"openai-completions","baseUrl":"$host","provider":"$provider","reasoning":true,"input":["text"],"cost":{"input":0.3,"output":1.2,"cacheRead":0.006,"cacheWrite":0},"contextWindow":1000000,"maxTokens":384000,"compat":{"supportsStore":false,"supportsDeveloperRole":false,"maxTokensField":"max_tokens","requiresReasoningContentOnAssistantMessages":true,"thinkingFormat":"deepseek"},"thinkingLevelMap":{"minimal":null,"low":"low","medium":null,"high":"high","max":"max"}}]}}}
EOF
  HOME=$home PI_CODING_AGENT_DIR=$home/pi DEEPSEEK_API_KEY=$key \
    "$tools/node/bin/node" "$tools/pi/lib/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js" \
    --print --mode json --provider "$provider" --model "$BENCH_MODEL" ${BENCH_EFFORT:+--thinking "$BENCH_EFFORT"} "$prompt" </dev/null
  ;;
omp)
  mkdir -p "$home/.omp/agent"
  printf 'providers:\n  %s:\n    baseUrl: %s\n    models:\n      %s:\n        api: openai-completions\n' "$provider" "$host" "$BENCH_MODEL" > "$home/.omp/agent/models.yml"
  HOME=$home DEEPSEEK_API_KEY=$key "$tools/bun/bun" "$tools/omp/lib/node_modules/@oh-my-pi/pi-coding-agent/dist/cli.js" \
    -p --mode json --no-session --yolo --model "$provider/$BENCH_MODEL" ${BENCH_EFFORT:+--thinking "$BENCH_EFFORT"} "$prompt" </dev/null
  ;;
opencode2)
  mkdir -p "$home/.config/opencode"
  cat > "$home/.config/opencode/opencode.json" <<EOF
{"\$schema": "https://opencode.ai/config.json", "provider": {"$provider": {"options": {"baseURL": "$host"}}}}
EOF
  HOME=$home XDG_CONFIG_HOME=$home/.config XDG_DATA_HOME=$home/.local/share OPENCODE_DISABLE_MODELS_FETCH=1 DEEPSEEK_API_KEY=$key \
    "$tools/opencode/bin/opencode2" run --format json --model "$provider/$BENCH_MODEL" ${BENCH_EFFORT:+--variant "$BENCH_EFFORT"} \
    --dangerously-skip-permissions -- "$prompt" </dev/null
  ;;
hermes)
  mkdir -p "$home/hermes"
  printf 'model:\n  provider: %s\n  default: %s\n' "$provider" "$BENCH_MODEL" > "$home/hermes/config.yaml"
  HOME=$home HERMES_HOME=$home/hermes DEEPSEEK_API_KEY=$key DEEPSEEK_BASE_URL=$BENCH_BASE \
    "$tools/hermes/.venv/bin/hermes" --yolo chat -q "$prompt" -Q ${BENCH_EFFORT:+--reasoning "$BENCH_EFFORT"} </dev/null
  ;;
mcode)
  mkdir -p "$home/.minimax"
  printf 'permissionMode: bypassPermissions\n' > "$home/.minimax/config.yaml"
  HOME=$home MINIMAX_DATA_DIR=$home/.minimax PATH="$tools/node/bin:$PATH" \
    MCODE_PROVIDER_API_KEY=$key \
    "$tools/mcode/bin/mcode" provider add --name bench --base-url "$BENCH_BASE" \
    --api-format openai-completions --model "$BENCH_MODEL" --api-key-env MCODE_PROVIDER_API_KEY
  HOME=$home MINIMAX_DATA_DIR=$home/.minimax PATH="$tools/node/bin:$PATH" \
    MCODE_PROVIDER_API_KEY=$key \
    "$tools/mcode/bin/mcode" exec --output-format json --permission full --model "bench/$BENCH_MODEL" \
    ${BENCH_EFFORT:+--effort "$BENCH_EFFORT"} "$prompt" </dev/null
  ;;
claude)
  HOME=$home ANTHROPIC_BASE_URL="$host/anthropic" ANTHROPIC_AUTH_TOKEN=$key \
    ANTHROPIC_MODEL=claude-sonnet-4-5 ANTHROPIC_DEFAULT_OPUS_MODEL=claude-sonnet-4-5 \
    ANTHROPIC_DEFAULT_SONNET_MODEL=claude-sonnet-4-5 ANTHROPIC_DEFAULT_HAIKU_MODEL=claude-haiku-4-5 \
    CLAUDE_CODE_SUBAGENT_MODEL=$BENCH_MODEL CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
    IS_SANDBOX=1 \
    "$tools/claude/bin/claude" -p --dangerously-skip-permissions --output-format json ${BENCH_EFFORT:+--effort "$BENCH_EFFORT"} "$prompt"
  ;;
codex)
  mkdir -p "$home/.codex"
  cat > "$home/.codex/config.toml" <<EOF
model = "$BENCH_MODEL"
model_provider = "bench"
[model_providers.bench]
name = "bench"
base_url = "$BENCH_BASE"
env_key = "DEEPSEEK_API_KEY"
wire_api = "chat"
EOF
  HOME=$home CODEX_HOME=$home/.codex DEEPSEEK_API_KEY=$key \
    "$tools/codex/bin/codex" exec --skip-git-repo-check --sandbox danger-full-access --json "$prompt"
  ;;
grok)
  mkdir -p "$home/.grok"
  cat > "$home/.grok/config.toml" <<EOF
[models]
default = "flash"
[model.flash]
model = "$BENCH_MODEL"
base_url = "$BENCH_BASE"
name = "bench"
env_key = "DEEPSEEK_API_KEY"
api_backend = "chat_completions"
EOF
  HOME=$home DEEPSEEK_API_KEY=$key \
    "$tools/grok/bin/grok" --single "$prompt" --output-format json -m flash --always-approve --cwd .
  ;;
*)
  echo "unknown harness: $name" >&2
  exit 2
  ;;
esac
