#!/bin/sh
# Cross-build ONE app for the ZPOD (ARMv6) and pack it: the build step of the
# dev loop (scripts/zpod-dev, docs/DEV-LOOP.md). Runs where cargo-zigbuild is
# (the zpod-br container, or a Mac/Linux host with the musl target).
#   tools/build-app.sh <id> [out-dir]    -> <out-dir>/<id>-<version>.zpa (default dist/dev)
#   tools/build-app.sh --core [out-dir]  -> <out-dir>/core/{zpod-launcher,zpod-app}
# <id> needs apps/packages/<id>/manifest.toml (+ icon.png, any assets); the
# binary is the crate "zpod-<id>" (or "<id>"; override with ZPOD_APP_PKG)
# whose bin is named like the manifest's exec.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd); cd "$here"
T=${TARGET:-arm-unknown-linux-musleabihf}
R=target/$T/release
[ $# -ge 1 ] || { sed -n '5,9p' "$0"; exit 2; }
out=${2:-dist/dev}
mkdir -p "$out"

check_arch() {
	command -v readelf >/dev/null 2>&1 || return 0   # zpod-app install checks it again on the device
	arch=$(readelf -A "$1" | sed -n 's/.*Tag_CPU_arch: //p')
	case "$arch" in v6|v6KZ|v6K) ;; *) echo "$1: Tag_CPU_arch=$arch, not ARMv6" >&2; exit 1 ;; esac
}

if [ "$1" = --core ]; then
	cargo zigbuild --release --target "$T" -p zpod-launcher -p zpod-apps
	mkdir -p "$out/core"
	for b in zpod-launcher zpod-app; do check_arch "$R/$b"; cp "$R/$b" "$out/core/$b"; done
	ls -l "$out/core"
	exit 0
fi

id=$1
src=packages/$id
[ -f "$src/manifest.toml" ] || { echo "no $src/manifest.toml" >&2; exit 1; }
exec_rel=$(sed -n 's/^exec *= *"\(.*\)".*/\1/p' "$src/manifest.toml" | head -1)
bin=${exec_rel##*/}
pkg=${ZPOD_APP_PKG:-}
if [ -z "$pkg" ]; then
	if grep -qs "^name = \"zpod-$id\"" crates/*/Cargo.toml; then pkg=zpod-$id; else pkg=$id; fi
fi
cargo zigbuild --release --target "$T" -p "$pkg" --bin "$bin"
check_arch "$R/$bin"
stage=$out/stage/$id
rm -rf "$stage"; mkdir -p "$stage"
cp -R "$src/." "$stage/"
mkdir -p "$stage/$(dirname "$exec_rel")"
cp "$R/$bin" "$stage/$exec_rel"
# host zpod-app for pack (a no-op rebuild when nothing changed)
cargo build --release -q -p zpod-apps
rm -f "$out/$id"-*.zpa
SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-1792195200} target/release/zpod-app pack "$stage" "$out"
