#!/bin/sh
# Cross-build all ZPOD M4 binaries for the Pi Zero W (ARMv6, arm1176jzf-s),
# verify the ELF arch, and stage dist/:
#   dist/core/usr/lib/zpod/bin/{zpod-launcher,zpod-bootsplash,zpod-drop,zpod-app}
#   dist/packages/{music,clock,tetris}-<ver>.zpa
# Needs: rustup target arm-unknown-linux-musleabihf, cargo-zigbuild + zig.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd); cd "$here"
T=${TARGET:-arm-unknown-linux-musleabihf}
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2}
nice -n 10 cargo zigbuild --release --target "$T" \
  -p zpod-launcher -p zpod-bootsplash -p zpod-drop -p zpod-apps -p zpod-music -p zpod-clock -p zpod-tetris
R=target/$T/release
rm -rf dist; mkdir -p dist/core/usr/lib/zpod/bin dist/packages dist/stage
fail=0
for b in zpod-launcher zpod-bootsplash zpod-drop zpod-app music clock tetris; do
  arch=$(readelf -A "$R/$b" | sed -n 's/.*Tag_CPU_arch: //p')
  printf '%-16s %8s bytes  Tag_CPU_arch=%s\n' "$b" "$(stat -c %s "$R/$b")" "$arch"
  case "$arch" in v6|v6KZ|v6K) ;; *) echo "  NOT ARMv6!"; fail=1 ;; esac
done
[ $fail = 0 ] || exit 1
for b in zpod-launcher zpod-bootsplash zpod-drop zpod-app; do cp "$R/$b" dist/core/usr/lib/zpod/bin/; done
host_app=target/release/zpod-app
[ -x "$host_app" ] || nice -n 10 cargo build --release -p zpod-apps
for a in music clock tetris; do
  mkdir -p "dist/stage/$a/bin"
  cp "packages/$a/manifest.toml" "packages/$a/icon.png" "dist/stage/$a/"
  cp "$R/$a" "dist/stage/$a/bin/$a"
  SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-1792195200} "$host_app" pack "dist/stage/$a" dist/packages >/dev/null
done
ls -l dist/packages
