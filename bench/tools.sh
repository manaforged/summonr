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

docker run --rm -v "$out":/opt/bench \
  ${src:+-v "$src":/src:ro} \
  ${sigil:+-v "$sigil":/sigil:ro} \
  -v "$HOME/.cargo/git:/root/.cargo/git" \
  -v "$HOME/.cargo/registry:/root/.cargo/registry" \
  -v "$HOME/.ssh:/root/.ssh:ro" \
  -w /opt/bench debian:bookworm bash -euo pipefail -c "
export CARGO_NET_GIT_FETCH_WITH_CLI=true
apt-get update -qq >/dev/null
apt-get install -y -qq curl ca-certificates git openssh-client make cmake clang libclang-dev g++ pkg-config libssl-dev >/dev/null
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

if [ -n "$src" ] && [ ! -x "$out/summonr/bin/summonr" ]; then
  echo "missing tool entrypoint: summonr/bin/summonr" >&2
  exit 1
fi
cat > "$out/versions.txt" <<EOF
summonr $(sha256sum "$out/summonr/bin/summonr" | cut -d' ' -f1)
EOF
if [ -n "$src" ] && git -C "$src" rev-parse HEAD >/dev/null 2>&1; then
  echo "summonr_commit $(git -C "$src" rev-parse HEAD)" >> "$out/versions.txt"
fi
here=$(cd "$(dirname "$0")" && pwd)
pi=$(awk '$1=="pi"{print $2}' "$here/tools.lock")
if [ -x "$out/node/bin/node" ] && [ -n "$pi" ]; then
  PATH="$out/node/bin:$PATH" npm install -g --silent --prefix "$out/pi" "@earendil-works/pi-coding-agent@$pi"
  echo "node $("$out/node/bin/node" --version | tr -d v)" >> "$out/versions.txt"
  echo "pi $("$out/node/bin/node" -p "require('$out/pi/lib/node_modules/@earendil-works/pi-coding-agent/package.json').version")" >> "$out/versions.txt"
fi
install -m 755 "$here/harness.sh" "$out/harness.sh"
(cd "$out" && find . -type f \( -perm -u+x -path './*/bin/*' -o -name cacert.pem -o -name harness.sh \) -print0 | sort -z | xargs -0 sha256sum) > "$out/manifest.sha256"
echo "tools ready in $out"
