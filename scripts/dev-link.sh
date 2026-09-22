#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
bindir=${SUMMON_BIN_DIR:-$HOME/.cargo/bin}
stamp=$(date +%Y%m%d%H%M%S)

cargo build --locked --manifest-path "$root/Cargo.toml" --release \
    --bin summonr

mkdir -p "$bindir"
for name in summonr summon summoner; do
    link="$bindir/$name"
    if [ -e "$link" ] && [ ! -L "$link" ]; then
        mv "$link" "$link.bak-$stamp"
    fi
    ln -sfn "$root/target/release/summonr" "$link"
done

"$bindir/summon" -V
