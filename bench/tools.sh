#!/usr/bin/env bash
set -euo pipefail

out=${1:?usage: tools.sh OUT_DIR [SUMMONR_SRC]}
src=${2:-}
sigil=${SIGIL:-}
mkdir -p "$out"
out=$(cd "$out" && pwd)
if [ -n "$src" ]; then
  src=$(cd "$src" && pwd)
fi
if [ -n "$sigil" ]; then
  sigil=$(cd "$sigil" && pwd)
fi

here=$(cd "$(dirname "$0")" && pwd)
pin() { awk -v k="$1" '$1==k{print $2}' "$here/tools.lock"; }
hermes=$(pin hermes) uv=$(pin uv) bun=$(pin bun)

if [ -n "$src" ]; then
  docker run --rm -v "$out":/opt/tools -v "$src":/src:ro \
    ${sigil:+-v "$sigil":/sigil:ro} \
    -v "$HOME/.cargo/git:/root/.cargo/git" \
    -v "$HOME/.cargo/registry:/root/.cargo/registry" \
    "alpine:$(pin alpine)" sh -euc "
apk add -q --no-cache curl ca-certificates git bash make cmake clang clang-dev g++ musl-dev pkgconf linux-headers perl >/dev/null
curl -fsSL https://sh.rustup.rs | sh -s -- -y --default-toolchain stable >/dev/null
. /root/.cargo/env
export CARGO_NET_GIT_FETCH_WITH_CLI=true CC_x86_64_unknown_linux_musl=gcc CXX_x86_64_unknown_linux_musl=g++
cargo build --manifest-path /src/Cargo.toml --release --locked -p summonr --bin summonr --target x86_64-unknown-linux-musl --target-dir /tmp/summonr-target
install -D -m 755 /tmp/summonr-target/x86_64-unknown-linux-musl/release/summonr /opt/tools/summonr/bin/summonr
"
fi

docker run --rm -v "$out":/opt/tools \
  -e HERMES="$hermes" -e UV="$uv" -e BUN="$bun" -e OWNER="$(id -u):$(id -g)" \
  ${src:+-v "$src":/src:ro} \
  ${sigil:+-v "$sigil":/sigil:ro} \
  -v "$HOME/.cargo/git:/root/.cargo/git" \
  -v "$HOME/.cargo/registry:/root/.cargo/registry" \
  -w /opt/tools debian:bookworm bash -euo pipefail -c "
export CARGO_NET_GIT_FETCH_WITH_CLI=true
apt-get update -qq >/dev/null
apt-get install -y -qq curl ca-certificates git unzip make cmake clang libclang-dev g++ pkg-config libssl-dev >/dev/null
install -m 644 /etc/ssl/certs/ca-certificates.crt cacert.pem
if [ -n \"\$HERMES\" ] && [ -n \"\$UV\" ]; then
  rm -rf hermes hermes-python uv && mkdir -p uv
  curl -fsSL https://github.com/astral-sh/uv/releases/download/\$UV/uv-x86_64-unknown-linux-gnu.tar.gz | tar -xz --strip-components=1 -C uv
  git clone --quiet --depth 1 --branch \$HERMES https://github.com/NousResearch/hermes-agent hermes
  (cd hermes && UV_PYTHON_INSTALL_DIR=/opt/tools/hermes-python ../uv/uv sync --quiet --frozen --python 3.14)
fi
if [ -n \"\$BUN\" ]; then
  rm -rf bun /tmp/bun.zip && mkdir -p bun
  curl -fsSL -o /tmp/bun.zip https://github.com/oven-sh/bun/releases/download/bun-v\$BUN/bun-linux-x64.zip
  unzip -q -j /tmp/bun.zip -d bun && chmod 755 bun/bun
fi
chown -R \"\$OWNER\" /opt/tools
"

if [ -n "$src" ] && [ ! -x "$out/summonr/bin/summonr" ]; then
  echo "missing tool entrypoint: summonr/bin/summonr" >&2
  exit 1
fi
cat > "$out/versions.txt" <<EOF
summonr $(sha256sum "$out/summonr/bin/summonr" | cut -d' ' -f1)
EOF
if [ -n "$src" ] && git -C "$src" rev-parse HEAD >/dev/null 2>&1; then
  echo "summonr_commit $(git -C "$src" rev-parse HEAD)$([ -z "$(git -C "$src" status --porcelain)" ] || echo -dirty)" >> "$out/versions.txt"
fi
if [ -x "$out/summonr/bin/summonr" ]; then
  echo "summonr_version $("$out/summonr/bin/summonr" --version | awk '{print $2}')" >> "$out/versions.txt"
fi
pi=$(pin pi) opencode2=$(pin opencode2) omp=$(pin omp) codex=$(pin codex) claude=$(pin claude) grok=$(pin grok)
if [ -x "$out/node/bin/node" ] && [ -n "$pi" ]; then
  PATH="$out/node/bin:$PATH" npm install -g --silent --prefix "$out/pi" "@earendil-works/pi-coding-agent@$pi"
  echo "node $("$out/node/bin/node" --version | tr -d v)" >> "$out/versions.txt"
  echo "pi $("$out/node/bin/node" -p "require('$out/pi/lib/node_modules/@earendil-works/pi-coding-agent/package.json').version")" >> "$out/versions.txt"
fi
if [ -x "$out/node/bin/node" ] && [ -n "$opencode2" ]; then
  rm -rf "$out/opencode"
  PATH="$out/node/bin:$PATH" npm install -g --silent --prefix "$out/opencode" "@opencode/cli@$opencode2"
  echo "opencode2 $("$out/node/bin/node" -p "require('$out/opencode/lib/node_modules/@opencode/cli/package.json').version")" >> "$out/versions.txt"
fi
if [ -x "$out/node/bin/node" ] && [ -n "$omp" ]; then
  rm -rf "$out/omp"
  PATH="$out/node/bin:$PATH" npm install -g --silent --prefix "$out/omp" "@oh-my-pi/pi-coding-agent@$omp"
  echo "omp $("$out/node/bin/node" -p "require('$out/omp/lib/node_modules/@oh-my-pi/pi-coding-agent/package.json').version")" >> "$out/versions.txt"
  echo "bun $("$out/bun/bun" --version)" >> "$out/versions.txt"
fi
if [ -x "$out/node/bin/node" ] && [ -n "$codex" ]; then
  rm -rf "$out/codex" "$out/codex-npm"
  PATH="$out/node/bin:$PATH" npm install -g --silent --prefix "$out/codex-npm" "@openai/codex@$codex"
  platform=$("$out/node/bin/node" -p "require('path').dirname(require.resolve('@openai/codex-linux-x64/package.json', {paths: ['$out/codex-npm/lib/node_modules/@openai/codex']}))")
  cp -R "$platform/vendor/x86_64-unknown-linux-musl" "$out/codex"
  rm -rf "$out/codex-npm"
  echo "codex $("$out/codex/bin/codex" --version | awk '{print $2}')" >> "$out/versions.txt"
fi
if [ -x "$out/node/bin/node" ] && [ -n "$claude" ]; then
  rm -rf "$out/claude"
  PATH="$out/node/bin:$PATH" npm install -g --silent --prefix "$out/claude" "@anthropic-ai/claude-code@$claude"
  echo "claude $("$out/claude/bin/claude" --version | awk '{print $1}')" >> "$out/versions.txt"
fi
if [ -x "$out/node/bin/node" ] && [ -n "$grok" ]; then
  rm -rf "$out/grok"
  PATH="$out/node/bin:$PATH" npm install -g --silent --prefix "$out/grok" "@xai-official/grok@$grok"
  echo "grok $(PATH="$out/node/bin:$PATH" "$out/grok/bin/grok" --version | awk '{print $2}')" >> "$out/versions.txt"
fi
if [ -n "$hermes" ]; then
  [ -x "$out/hermes/.venv/bin/hermes" ] || { echo "missing tool entrypoint: hermes/.venv/bin/hermes" >&2; exit 1; }
  echo "hermes $(git -C "$out/hermes" describe --tags --exact-match 2>/dev/null || echo "$hermes")" >> "$out/versions.txt"
fi
install -m 755 "$here/harness.sh" "$out/harness.sh"
(cd "$out" && find . -type f \( -perm -u+x -path './*/bin/*' -o -name cacert.pem -o -name harness.sh \) -print0 | sort -z | xargs -0 sha256sum) > "$out/manifest.sha256"
echo "tools ready in $out"
