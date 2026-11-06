#!/bin/sh
# Usage: scripts/package-macos.sh EXE OUT_DIR
# Needs sigil-pack on PATH. Set SIGIL_CODESIGN_IDENTITY to sign with a
# Developer ID, and SIGIL_NOTARY_PROFILE to notarize.
set -eu
exe=$1
out=$2
assets=$(dirname "$0")/../desktop/assets
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$(dirname "$0")/../Cargo.toml" | head -n 1)
mkdir -p "$out"
sigil-pack macos --exe "$exe" --out "$out/Summonr.app" --name Summonr \
  --exe-name summonr-desktop --id dev.summonr.app --version "$version" \
  --min 11.0 --icon "$assets/icon.icns"
sigil-pack dmg --app "$out/Summonr.app" --out "$out/Summonr.dmg" \
  --background "$assets/dmg-background.tiff" \
  --layout "$assets/dmg-layout.DS_Store" --icon "$assets/icon.icns"
if [ -n "${SIGIL_NOTARY_PROFILE:-}" ]; then
  sigil-pack notarize --file "$out/Summonr.dmg"
fi
