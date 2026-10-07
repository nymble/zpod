#!/usr/bin/env bash
# Build aoide-zpod-dac-dkms_<ver>_all.deb (DKMS source + compiled overlay).
# Usage: build-deb.sh <outdir>
# Needs: dpkg-deb, dtc. Runs on the build host; the deb is installed in the
# pi-gen chroot, where its postinst builds the module for every installed
# kernel that has headers (rpi-v6 / rpi-v7). DKMS rebuilds on kernel upgrades.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT="${1:?usage: $0 <outdir>}"
NAME=aoide-zpod-dac
VER=1.0.0
DEBREV=1
STAGE="$(mktemp -d)"
chmod 755 "$STAGE"
trap 'rm -rf "$STAGE"' EXIT

SRC="$STAGE/usr/src/${NAME}-${VER}"
install -d "$SRC" "$STAGE/boot/firmware/overlays" "$STAGE/DEBIAN" \
  "$STAGE/usr/share/doc/${NAME}-dkms"
install -m 644 "$HERE/aoide-zpod-dac.c" "$HERE/Makefile" "$SRC/"
sed "s/#VERSION#/${VER}/" "$HERE/dkms.conf" > "$SRC/dkms.conf"
chmod 644 "$SRC/dkms.conf"
dtc -@ -q -I dts -O dtb -o "$STAGE/boot/firmware/overlays/${NAME}.dtbo" \
  "$HERE/aoide-zpod-dac-overlay.dts"
chmod 644 "$STAGE/boot/firmware/overlays/${NAME}.dtbo"
install -m 644 "$HERE/aoide-zpod-dac-overlay.dts" "$HERE/README.md" \
  "$STAGE/usr/share/doc/${NAME}-dkms/"

cat > "$STAGE/DEBIAN/control" <<CTRL
Package: ${NAME}-dkms
Version: ${VER}-${DEBREV}
Architecture: all
Maintainer: nymble <764245+nymble@users.noreply.github.com>
Section: kernel
Priority: optional
Depends: dkms (>= 3.0)
Recommends: linux-headers-rpi-v6 | linux-headers-rpi-v7
Homepage: https://github.com/nymble/zpod
Description: ASoC machine driver + overlay for the Aoide ZPOD DAC (PCM5122)
 Out-of-tree GPL-2.0 rebuild of the vendor aoide-zpod-dac driver for current
 Raspberry Pi OS kernels, packaged for DKMS so it is rebuilt automatically on
 kernel upgrades. Also installs /boot/firmware/overlays/aoide-zpod-dac.dtbo.
 Enable with: dtparam=i2s=on, dtparam=i2c_arm=on, dtoverlay=aoide-zpod-dac.
CTRL

cat > "$STAGE/DEBIAN/postinst" <<POST
#!/bin/sh
set -e
NAME=${NAME}
VER=${VER}
if [ "\$1" = "configure" ]; then
  if ! dkms status -m "\$NAME" -v "\$VER" 2>/dev/null | grep -q .; then
    dkms add -m "\$NAME" -v "\$VER"
  fi
  # Build for every installed kernel that has headers (not just uname -r,
  # which is the build host's kernel inside a pi-gen chroot).
  for d in /lib/modules/*; do
    k=\$(basename "\$d")
    if [ -e "\$d/build/Makefile" ]; then
      dkms install -m "\$NAME" -v "\$VER" -k "\$k" || echo "warning: dkms build failed for \$k" >&2
    else
      echo "note: no headers for \$k; skipping \$NAME" >&2
    fi
  done
fi
exit 0
POST
cat > "$STAGE/DEBIAN/prerm" <<PRERM
#!/bin/sh
set -e
if [ "\$1" = "remove" ] || [ "\$1" = "upgrade" ] || [ "\$1" = "deconfigure" ]; then
  dkms remove -m ${NAME} -v ${VER} --all >/dev/null 2>&1 || true
fi
exit 0
PRERM
chmod 755 "$STAGE/DEBIAN/postinst" "$STAGE/DEBIAN/prerm"

mkdir -p "$OUT"
DEB="$OUT/${NAME}-dkms_${VER}-${DEBREV}_all.deb"
dpkg-deb --root-owner-group -Zxz --build "$STAGE" "$DEB" >/dev/null
echo "$DEB"
