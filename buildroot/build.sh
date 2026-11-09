#!/bin/sh
# Build the ZPOD Buildroot image (Linux build host or container; never flashes).
#
#   buildroot/build.sh                 apps + full image -> $ZPOD_BR_WORK/output/images/sdcard.img
#   buildroot/build.sh apps            only cross-build the Rust apps into apps/dist
#   buildroot/build.sh <make target>   e.g. menuconfig, linux-menuconfig, mpd-rebuild
#
# Env: ZPOD_BR_WORK (default /br): Buildroot checkout, dl/ and output/ live
#      there (must be a case-sensitive Linux filesystem, ~25 GB free).
#      BR_VERSION (default 2026.02.3).  BR2_JLEVEL (default: nproc).
# Needs: Buildroot host deps (build-essential, git, wget, cpio, unzip, rsync,
#        bc, file, python3, libncurses-dev, libssl-dev), plus for the apps
#        rustup (target arm-unknown-linux-musleabihf), zig and cargo-zigbuild.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/.." && pwd)
WORK=${ZPOD_BR_WORK:-/br}
BR_VERSION=${BR_VERSION:-2026.02.3}
BR=$WORK/buildroot
OUT=$WORK/output
export BR2_DL_DIR=${BR2_DL_DIR:-$WORK/dl}
mkdir -p "$WORK" "$BR2_DL_DIR"

build_apps() {
	# Build in a private copy so a macOS checkout's apps/target is never mixed in.
	[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
	mkdir -p "$WORK/apps-build"
	rsync -a --delete --exclude target --exclude dist "$REPO/apps/" "$WORK/apps-build/"
	(cd "$WORK/apps-build" && CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-4} sh tools/build-arm.sh)
	rm -rf "$REPO/apps/dist"; cp -a "$WORK/apps-build/dist" "$REPO/apps/dist"
}

if [ ! -d "$BR/.git" ]; then
	git clone --depth 1 -b "$BR_VERSION" https://gitlab.com/buildroot.org/buildroot.git "$BR"
fi

case "${1:-all}" in
apps) build_apps; exit 0 ;;
all) build_apps; set -- ;;
esac

export ZPOD_HOST_APP="$WORK/apps-build/target/release/zpod-app"
make -C "$BR" BR2_EXTERNAL="$HERE" O="$OUT" zpod_pi0w_defconfig >/dev/null
exec make -C "$BR" BR2_EXTERNAL="$HERE" O="$OUT" BR2_JLEVEL="${BR2_JLEVEL:-$(nproc)}" "$@"
