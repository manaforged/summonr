#!/usr/bin/env bash
set -euo pipefail

NODE=22.19.0
BUN=1.4.2
PI=0.85.1
OMP=18.2.2
OPENCODE=2.0.5
MODELS_DEV=https://models.opencode.ai/api.json
HERMES=v2026.9.14
UV=0.12.15

out=${1:?usage: tools.sh OUT_DIR SUMMONR_BINARY}
summonr=${2:?usage: tools.sh OUT_DIR SUMMONR_BINARY}
mkdir -p "$out"
out=$(cd "$out" && pwd)

install -D -m 755 "$summonr" "$out/summonr/bin/summonr"

docker run --rm -v "$out":/opt/bench -w /opt/bench debian:bookworm bash -euo pipefail -c "
apt-get update -qq >/dev/null
apt-get install -y -qq curl ca-certificates xz-utils bzip2 unzip git >/dev/null

rm -rf node && mkdir node
curl -fsSL https://nodejs.org/dist/v$NODE/node-v$NODE-linux-x64.tar.xz | tar -xJ --strip-components=1 -C node

rm -rf pi && mkdir -p pi
PATH=/opt/bench/node/bin:\$PATH npm install -g --prefix /opt/bench/pi --ignore-scripts @earendil-works/pi-coding-agent@$PI >/dev/null

rm -rf bun && mkdir bun
curl -fsSL -o /tmp/bun.zip https://github.com/oven-sh/bun/releases/download/bun-v$BUN/bun-linux-x64.zip
unzip -q -j /tmp/bun.zip -d bun
rm -rf omp && mkdir -p omp
PATH=/opt/bench/node/bin:/opt/bench/bun:\$PATH npm install -g --prefix /opt/bench/omp @oh-my-pi/pi-coding-agent@$OMP >/dev/null

rm -rf opencode && mkdir -p opencode
curl -fsSL https://registry.npmjs.org/@opencode/cli-linux-x64/-/cli-linux-x64-$OPENCODE.tgz | tar -xz -C opencode --strip-components=1 package/bin/opencode
curl -fsSL -o opencode/models.json $MODELS_DEV

rm -rf hermes hermes-python uv && mkdir -p uv
curl -fsSL https://github.com/astral-sh/uv/releases/download/$UV/uv-x86_64-unknown-linux-gnu.tar.gz | tar -xz --strip-components=1 -C uv
git clone --quiet --depth 1 --branch $HERMES https://github.com/NousResearch/hermes-agent hermes
cd hermes && UV_PYTHON_INSTALL_DIR=/opt/bench/hermes-python ../uv/uv sync --quiet --frozen --python 3.12 && cd ..
install -m 644 /etc/ssl/certs/ca-certificates.crt cacert.pem
"

cat > "$out/versions.txt" <<EOF
node $NODE
bun $BUN
pi $PI
omp $OMP
opencode $OPENCODE
opencode-models $MODELS_DEV $(sha256sum "$out/opencode/models.json" | cut -d' ' -f1) $(date -u +%F)
hermes $HERMES
uv $UV
summonr $(sha256sum "$summonr" | cut -d' ' -f1)
EOF
(cd "$out" && find . -type f \( -perm -u+x \( -path './*/bin/*' -o -name 'cli.js' \) -o -name cacert.pem -o -path ./opencode/models.json \) -print0 | sort -z | xargs -0 sha256sum) > "$out/manifest.sha256"
echo "tools ready in $out"
