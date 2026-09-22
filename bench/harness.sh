#!/usr/bin/env bash
set -euo pipefail

name=${1:?usage: harness.sh NAME PROMPT_FILE}
prompt=$(cat "${2:?usage: harness.sh NAME PROMPT_FILE}")
: "${BENCH_BASE:?BENCH_BASE is the proxy URL ending in /v1}"
: "${BENCH_MODEL:?}"
key=${BENCH_API_KEY:-${DEEPSEEK_API_KEY:?}}
tools=${BENCH_TOOLS:-/opt/bench}
home=$(mktemp -d /tmp/bench-home.XXXXXX)
logs=${BENCH_LOGS:-/logs/agent}
if [[ $BENCH_BASE == *host.docker.internal* ]]; then
  gw=$(python3 -c "import pathlib; g=pathlib.Path('/proc/net/route').read_text().splitlines()[1].split()[2]; b=bytes.fromhex(g); print('.'.join(str(x) for x in b[::-1]))")
  BENCH_BASE=${BENCH_BASE//host.docker.internal/$gw}
fi
export DEEPSEEK_API_KEY=$key
unset ANTHROPIC_API_KEY OPENAI_API_KEY XAI_API_KEY CLAUDE_CODE_OAUTH_TOKEN CHATGPT_API_KEY || true
[ -s /etc/ssl/certs/ca-certificates.crt ] || export SSL_CERT_FILE=$tools/cacert.pem
mkdir -p "$logs"

case "$name" in
summonr)
  mkdir -p "$logs/summonr"
  HOME=$home SUMMONR_HOME=$logs/summonr SUMMONR_BASE_URL=$BENCH_BASE SUMMONR_MODEL=$BENCH_MODEL SUMMONR_API_KEY=$key \
    "$tools/summonr/bin/summonr" -p "$prompt" --json ${BENCH_EFFORT:+--effort "$BENCH_EFFORT"}
  ;;
*)
  echo "unknown harness: $name" >&2
  exit 2
  ;;
esac
