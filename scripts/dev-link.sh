#!/bin/sh
# Point `summon` and `summoner` on PATH at this checkout, so the binaries you
# run are always the ones `cargo build` just wrote. Backs up the first copy it
# replaces. Run once per checkout; re-run after switching worktrees.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
bindir=${SUMMON_BIN_DIR:-$HOME/.cargo/bin}
stamp=$(date +%Y%m%d%H%M%S)

cargo build --locked --manifest-path "$root/Cargo.toml" --release \
    --bin summon --bin summoner

mkdir -p "$bindir"
for name in summon summoner; do
    link="$bindir/$name"
    if [ -e "$link" ] && [ ! -L "$link" ]; then
        mv "$link" "$link.bak-$stamp"
    fi
    ln -sfn "$root/target/release/$name" "$link"
done

"$bindir/summon" -V
