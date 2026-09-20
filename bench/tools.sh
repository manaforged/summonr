#!/usr/bin/env bash
set -euo pipefail

NODE=22.19.0
BUN=1.4.2
PI=0.85.1
OMP=18.2.2
OPENCODE=2.0.10
HERMES=v2026.9.14
MCODE=0.4.12
CLAUDE=2.1.267
CODEX=0.155.1
GROK=1.0.34
UV=0.12.15

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

docker run --rm -v "$out":/opt/bench \
  ${src:+-v "$src":/src:ro} \
  ${sigil:+-v "$sigil":/opt/sigil:ro} \
  -v "$HOME/.cargo/git:/root/.cargo/git" \
  -v "$HOME/.cargo/registry:/root/.cargo/registry" \
  -v "$HOME/.ssh:/root/.ssh:ro" \
  -w /opt/bench debian:bookworm bash -euo pipefail -c "
export CARGO_NET_GIT_FETCH_WITH_CLI=true
apt-get update -qq >/dev/null
apt-get install -y -qq curl ca-certificates xz-utils bzip2 unzip git openssh-client python3 make g++ pkg-config libssl-dev >/dev/null
arch=\$(uname -m)
case \$arch in
x86_64) node_arch=linux-x64; bun_arch=linux-x64; uv_arch=x86_64-unknown-linux-gnu ;;
aarch64) node_arch=linux-arm64; bun_arch=linux-aarch64; uv_arch=aarch64-unknown-linux-gnu ;;
*) echo \"unsupported arch: \$arch\" >&2; exit 1 ;;
esac

rm -rf node && mkdir node
curl -fsSL https://nodejs.org/dist/v$NODE/node-v$NODE-\$node_arch.tar.xz | tar -xJ --strip-components=1 -C node

rm -rf pi && mkdir -p pi
PATH=/opt/bench/node/bin:\$PATH npm install -g --prefix /opt/bench/pi --ignore-scripts @earendil-works/pi-coding-agent@$PI >/dev/null

rm -rf bun && mkdir bun
curl -fsSL -o /tmp/bun.zip https://github.com/oven-sh/bun/releases/download/bun-v$BUN/bun-\$bun_arch.zip
unzip -q -j /tmp/bun.zip -d bun
rm -rf omp && mkdir -p omp
PATH=/opt/bench/node/bin:/opt/bench/bun:\$PATH npm install -g --prefix /opt/bench/omp @oh-my-pi/pi-coding-agent@$OMP >/dev/null

rm -rf opencode && mkdir -p opencode
PATH=/opt/bench/node/bin:\$PATH npm install -g --prefix /opt/bench/opencode @opencode/cli@$OPENCODE >/dev/null

rm -rf hermes hermes-python uv && mkdir -p uv
curl -fsSL https://github.com/astral-sh/uv/releases/download/$UV/uv-\$uv_arch.tar.gz | tar -xz --strip-components=1 -C uv
git clone --quiet --depth 1 --branch $HERMES https://github.com/NousResearch/hermes-agent hermes
cd hermes && UV_PYTHON_INSTALL_DIR=/opt/bench/hermes-python ../uv/uv sync --quiet --frozen --python 3.12 && cd ..
rm -rf mcode && mkdir -p mcode
PATH=/opt/bench/node/bin:\$PATH npm install -g --prefix /opt/bench/mcode --include=optional @minimax-ai/code@$MCODE >/dev/null
rm -rf claude && mkdir -p claude
PATH=/opt/bench/node/bin:\$PATH npm install -g --prefix /opt/bench/claude @anthropic-ai/claude-code@$CLAUDE >/dev/null
rm -rf codex && mkdir -p codex
PATH=/opt/bench/node/bin:\$PATH npm install -g --prefix /opt/bench/codex @openai/codex@$CODEX >/dev/null
rm -rf grok && mkdir -p grok
PATH=/opt/bench/node/bin:\$PATH npm install -g --prefix /opt/bench/grok @xai-official/grok@$GROK >/dev/null
if [ -d /src ]; then
  mkdir -p /root/ssh2
  cp /root/.ssh/id_ed25519 /root/.ssh/known_hosts /root/ssh2/
  chmod 600 /root/ssh2/*
  export GIT_SSH_COMMAND=\"ssh -F /dev/null -i /root/ssh2/id_ed25519 -o UserKnownHostsFile=/root/ssh2/known_hosts\"
  git config --global url."git@github.com:manaforged/".insteadOf "https://github.com/manaforged/"
  curl -fsSL https://sh.rustup.rs | sh -s -- -y --default-toolchain stable >/dev/null
  . /root/.cargo/env
  cargo build --manifest-path /src/Cargo.toml --release --locked --target-dir /tmp/summonr-target
  install -D -m 755 /tmp/summonr-target/release/summonr /opt/bench/summonr/bin/summonr
fi
install -m 644 /etc/ssl/certs/ca-certificates.crt cacert.pem
"

for b in node/bin/node bun/bun pi/lib/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js omp/lib/node_modules/@oh-my-pi/pi-coding-agent/dist/cli.js opencode/bin/opencode2 hermes/.venv/bin/hermes mcode/bin/mcode claude/bin/claude codex/bin/codex grok/bin/grok uv/uv; do
  [ -x "$out/$b" ] || { echo "missing tool entrypoint: $b" >&2; exit 1; }
done
if [ -n "$src" ] && [ ! -x "$out/summonr/bin/summonr" ]; then
  echo "missing tool entrypoint: summonr/bin/summonr" >&2
  exit 1
fi
cat > "$out/versions.txt" <<EOF
node $NODE
bun $BUN
pi $PI
omp $OMP
opencode2 $OPENCODE
hermes $HERMES
mcode $MCODE
claude $CLAUDE
codex $CODEX
grok $GROK
uv $UV
summonr $(sha256sum "$out/summonr/bin/summonr" | cut -d' ' -f1)
EOF
if [ -n "$src" ] && git -C "$src" rev-parse HEAD >/dev/null 2>&1; then
  echo "summonr_commit $(git -C "$src" rev-parse HEAD)" >> "$out/versions.txt"
fi
install -m 755 "$(cd "$(dirname "$0")" && pwd)/harness.sh" "$out/harness.sh"
(cd "$out" && find . -type f \( -perm -u+x \( -path './*/bin/*' -o -name 'cli.js' \) -o -name cacert.pem -o -name harness.sh \) -print0 | sort -z | xargs -0 sha256sum) > "$out/manifest.sha256"
echo "tools ready in $out"
