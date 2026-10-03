#!/bin/sh
set -eu

repo=manaforged/summonr
releases="https://github.com/$repo/releases"
installer="https://summonr.dev/install.sh"
dir=${SUMMONR_INSTALL_DIR:-$HOME/.local/bin}
version=${SUMMONR_VERSION:-latest}
libc=${SUMMONR_LIBC:-musl}

fail() {
    echo "summonr install: $*" >&2
    exit 1
}

case $libc in
    musl | gnu) ;;
    *) fail "SUMMONR_LIBC must be musl or gnu, got $libc" ;;
esac

case $(uname -s) in
    Darwin) os=apple-darwin ;;
    Linux) os=unknown-linux-$libc ;;
    *) fail "unsupported OS $(uname -s). See $releases for available builds." ;;
esac

case $(uname -m) in
    x86_64 | amd64) arch=x86_64 ;;
    arm64 | aarch64) arch=aarch64 ;;
    *) fail "unsupported CPU $(uname -m). See $releases for available builds." ;;
esac

if [ "$os" = apple-darwin ] && [ "$arch" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
    arch=aarch64
fi
if [ "$os" = apple-darwin ] && [ "$arch" = x86_64 ]; then
    fail "macOS builds need Apple silicon; Intel Macs are not supported. See $releases for available builds."
fi

name="summonr-$arch-$os"
asset="$name.tar.gz"
if [ "$version" = latest ]; then
    base="$releases/latest/download"
else
    version="v${version#v}"
    base="$releases/download/$version"
fi

if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -q -O "$2" "$1"; }
else
    fail "needs curl or wget"
fi

if command -v sha256sum >/dev/null 2>&1; then
    digest() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
    digest() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
    fail "needs sha256sum or shasum to check the download"
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM

echo "Downloading $asset ($version)"
fetch "$base/$asset" "$work/$asset" || fail "cannot download $base/$asset. See $releases for available builds."
fetch "$base/$asset.sha256" "$work/$asset.sha256" || fail "cannot download $base/$asset.sha256"

want=$(cut -d' ' -f1 "$work/$asset.sha256")
got=$(digest "$work/$asset")
[ "$want" = "$got" ] || fail "checksum mismatch for $asset: expected $want, got $got. Nothing was installed."

tar -xzf "$work/$asset" -C "$work"
mkdir -p "$dir"
install -m 755 "$work/$name/summonr" "$dir/summonr"

installed=$("$dir/summonr" --version 2>/dev/null || echo "summonr $version")
echo "Installed $installed to $dir/summonr"

case ":$PATH:" in
    *":$dir:"*) ;;
    *) echo "Add $dir to PATH: export PATH=\"$dir:\$PATH\"" ;;
esac
echo "Update: curl -fsSL $installer | sh"
echo "Uninstall: rm $dir/summonr"
echo "Next: run summonr, then /login to pick a provider and paste a key."
