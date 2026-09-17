#!/usr/bin/env bash
set -euo pipefail

name=${1:?usage: harness.sh NAME PROMPT_FILE}
prompt=$(cat "${2:?usage: harness.sh NAME PROMPT_FILE}")
: "${BENCH_BASE:?BENCH_BASE is the proxy URL ending in /v1}"
: "${BENCH_MODEL:?}"
: "${ZAI_API_KEY:?}"
tools=${BENCH_TOOLS:-/opt/bench}
home=$(mktemp -d /tmp/bench-home.XXXXXX)
logs=${BENCH_LOGS:-/logs/agent}
[ -s /etc/ssl/certs/ca-certificates.crt ] || export SSL_CERT_FILE=$tools/cacert.pem
mkdir -p "$logs"

case "$name" in
summonr)
  mkdir -p "$logs/mage"
  printf '[modules]\nmcp = false\n\n[endpoint.zai]\nbase_url = "%s"\n' "$BENCH_BASE" > "$logs/mage/config.toml"
  HOME=$home MAGE_HOME=$logs/mage ZAI_API_KEY=$ZAI_API_KEY \
    "$tools/summonr/bin/summonr" -e zai --model "$BENCH_MODEL" -p "$prompt" --json ${BENCH_EFFORT:+--effort "$BENCH_EFFORT"}
  ;;
pi)
  mkdir -p "$home/pi"
  printf '{"providers": {"zai": {"baseUrl": "%s"}}}\n' "$BENCH_BASE" > "$home/pi/models.json"
  HOME=$home PI_CODING_AGENT_DIR=$home/pi ZAI_API_KEY=$ZAI_API_KEY \
    "$tools/node/bin/node" "$tools/pi/lib/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js" \
    --print --mode json --provider zai --model "$BENCH_MODEL" ${BENCH_EFFORT:+--thinking "$BENCH_EFFORT"} "$prompt" </dev/null
  ;;
omp)
  mkdir -p "$home/.omp/agent"
  printf 'providers:\n  zai:\n    baseUrl: %s\n' "$BENCH_BASE" > "$home/.omp/agent/models.yml"
  HOME=$home ZAI_API_KEY=$ZAI_API_KEY "$tools/bun/bun" "$tools/omp/lib/node_modules/@oh-my-pi/pi-coding-agent/dist/cli.js" \
    -p --mode json --no-session --yolo --model "zai/$BENCH_MODEL" ${BENCH_EFFORT:+--thinking "$BENCH_EFFORT"} "$prompt" </dev/null
  ;;
opencode)
  HOME=$home XDG_CONFIG_HOME=$home/.config XDG_DATA_HOME=$home/.local/share XDG_STATE_HOME=$home/.local/state \
    ZHIPU_API_KEY=$ZAI_API_KEY ZAI_API_KEY=$ZAI_API_KEY \
    OPENCODE_MODELS_PATH=$tools/opencode/models.json OPENCODE_DISABLE_MODELS_FETCH=1 OPENCODE_DISABLE_AUTOUPDATE=1 \
    OPENCODE_CONFIG_CONTENT="{\"providers\": {\"zai-coding-plan\": {\"settings\": {\"baseURL\": \"$BENCH_BASE\"}}}}" \
    "$tools/opencode/bin/opencode" run --standalone --auto --format json \
    --model "zai-coding-plan/$BENCH_MODEL${BENCH_EFFORT:+#$BENCH_EFFORT}" -- "$prompt" </dev/null
  ;;
hermes)
  mkdir -p "$home/hermes"
  printf 'model:\n  provider: zai\n  default: %s\n' "$BENCH_MODEL" > "$home/hermes/config.yaml"
  HOME=$home HERMES_HOME=$home/hermes GLM_API_KEY=$ZAI_API_KEY GLM_BASE_URL=$BENCH_BASE \
    "$tools/hermes/.venv/bin/hermes" --yolo chat -q "$prompt" -Q ${BENCH_EFFORT:+--reasoning "$BENCH_EFFORT"} </dev/null
  ;;
*)
  echo "unknown harness: $name" >&2
  exit 2
  ;;
esac
