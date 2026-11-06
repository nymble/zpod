#!/bin/bash
# ZPOD post-image: config.txt/cmdline.txt, seed the data partition with the
# bundled apps, and assemble sdcard.img (boot / rootA / rootB / data).
set -e
BOARD_DIR="$(dirname "$0")"
GENIMAGE_CFG="${BINARIES_DIR}/genimage-zpod.cfg"
GENIMAGE_TMP="${BUILD_DIR}/genimage.tmp"

install -m 0644 "${BOARD_DIR}/config.txt"  "${BINARIES_DIR}/rpi-firmware/config.txt"
install -m 0644 "${BOARD_DIR}/cmdline.txt" "${BINARIES_DIR}/rpi-firmware/cmdline.txt"
# Holding Home at power-on boots with cmdline-safe.txt (config.txt [gpio24=0]):
# the same command line plus zpod.nocore zpod.safe = no core update from /data
# for that boot (docs/BOOT-UPDATES.md).
printf '%s zpod.nocore zpod.safe\n' "$(tr -d '\n' < "${BOARD_DIR}/cmdline.txt")" > "${BINARIES_DIR}/rpi-firmware/cmdline-safe.txt"
chmod 0644 "${BINARIES_DIR}/rpi-firmware/cmdline-safe.txt"
# USB role switch, included from config.txt (gadget by default; docs/USB-HOST.md)
install -m 0644 "${BOARD_DIR}/zpod-usb-role.txt" "${BINARIES_DIR}/rpi-firmware/zpod-usb-role.txt"

# A/B systems (docs/AB-UPDATES.md): each system's kernel, device tree,
# overlays and command line (root=p2 for A, p3 for B) in slot-a/ / slot-b/;
# zpod-slot.txt (included last from config.txt) picks slot-a/. The top-level
# files stay as the no-prefix fallback for system A. Both slots start equal.
REL="${TARGET_DIR}/etc/zpod-release"
rel() { sed -n "s/^$1=//p" "$REL" 2>/dev/null | head -1; }
ABI=$(sed -n 's/^abi=//p' "${TARGET_DIR}/usr/share/zpod/core.id" 2>/dev/null)
for s in a b; do
	n=2; [ $s = b ] && n=3
	d="${BINARIES_DIR}/rpi-firmware/slot-$s"
	rm -rf "$d"; mkdir -p "$d"
	cp "${BINARIES_DIR}"/*.dtb "$d/"
	cp -R "${BINARIES_DIR}/rpi-firmware/overlays" "$d/overlays"
	sed "s#root=[^ ]*#root=/dev/mmcblk0p$n#; s/\$/ zpod.slot=$s/" "${BOARD_DIR}/cmdline.txt" | head -1 > "$d/cmdline.txt"
	printf '%s zpod.nocore zpod.safe\n' "$(cat "$d/cmdline.txt")" > "$d/cmdline-safe.txt"
	printf 'version=%s\ngit=%s\ndate=%s\nabi=%s\n' "$(rel ZPOD_VERSION)" "$(rel ZPOD_GIT)" "$(rel ZPOD_BUILD_DATE)" "${ABI:-1}" > "$d/VERSION"
	cp "${BINARIES_DIR}/zImage" "$d/zImage"
done
cat > "${BINARIES_DIR}/rpi-firmware/zpod-slot.txt" <<'SLOT'
# ZPOD A/B system choice, written by zpod-ab (docs/AB-UPDATES.md).
# Recovery on a computer: delete this file to boot system A from the
# top-level files. Holding Square while powering on boots the other one.
os_prefix=slot-a/
[gpio12=0]
os_prefix=slot-b/
[all]
SLOT
chmod -R u=rwX,go=rX "${BINARIES_DIR}/rpi-firmware"

# In-place update bundle for running cards (zpod-ab install; keeps /data):
# the root file system, this system's boot files, config.txt and zpod-ab
# itself (so a test5 card, which has no zpod-ab, can run it), with sha256s.
Z="${BINARIES_DIR}/zab"
rm -rf "$Z"; mkdir -p "$Z/boot"
cp "${BINARIES_DIR}/rootfs.squashfs" "$Z/"
cp "${BINARIES_DIR}/zImage" "${BINARIES_DIR}"/*.dtb "$Z/boot/"
cp -R "${BINARIES_DIR}/rpi-firmware/overlays" "$Z/boot/overlays"
cp "${BOARD_DIR}/cmdline.txt" "$Z/boot/cmdline.txt"
cp "${BINARIES_DIR}/rpi-firmware/config.txt" "$Z/boot/config.txt"
install -m 0755 "${TARGET_DIR}/usr/sbin/zpod-ab" "$Z/zpod-ab"
{
	printf 'format=1\nboard=pi0w\nversion=%s\ngit=%s\ndate=%s\nabi=%s\n' "$(rel ZPOD_VERSION)" "$(rel ZPOD_GIT)" "$(rel ZPOD_BUILD_DATE)" "${ABI:-1}"
	(cd "$Z" && find . -type f ! -name AB.manifest | LC_ALL=C sort | while read -r f; do echo "file=$(sha256sum "$f" | cut -d' ' -f1) ${f#./}"; done)
} > "$Z/AB.manifest"
rm -f "${BINARIES_DIR}/zpod-update.zab"
tar -C "$Z" --owner=0 --group=0 --numeric-owner -cf "${BINARIES_DIR}/zpod-update.zab" AB.manifest zpod-ab rootfs.squashfs boot
echo "post-image: A/B slot folders + zpod-update.zab ($(wc -c < "${BINARIES_DIR}/zpod-update.zab") bytes)"

FILES=()
for i in "${BINARIES_DIR}"/*.dtb "${BINARIES_DIR}"/rpi-firmware/*; do
	FILES+=( "${i#${BINARIES_DIR}/}" )
done
FILES+=( "zImage" )
BOOT_FILES=$(printf '\\t\\t\\t"%s",\\n' "${FILES[@]}")
sed "s|#BOOT_FILES#|${BOOT_FILES}|" "${BOARD_DIR}/genimage.cfg.in" > "${GENIMAGE_CFG}"

# Data partition seed: /data/zpod/apps/<id> pre-installed from the bundled
# .zpa packages with the host zpod-app (ZPOD_HOST_APP, set by buildroot/build.sh).
# Without it the device installs them on first boot (S00zpod-data).
trap 'rm -rf "${ROOTPATH_TMP}"' EXIT
ROOTPATH_TMP="$(mktemp -d)"
mkdir -p "${ROOTPATH_TMP}/zpod/apps" "${ROOTPATH_TMP}/zpod/appdata" "${ROOTPATH_TMP}/media/music" "${ROOTPATH_TMP}/media/playlists"
if [ -n "${ZPOD_HOST_APP:-}" ] && [ -x "${ZPOD_HOST_APP}" ]; then
	for a in "${TARGET_DIR}"/usr/share/zpod/packages/*.zpa; do
		ZPOD_APPS_DIR="${ROOTPATH_TMP}/zpod/apps" \
		ZPOD_CORE_VERSION_FILE="${TARGET_DIR}/etc/zpod/core-version" \
			"${ZPOD_HOST_APP}" install --force "$a"
	done
	echo "post-image: seeded /data/zpod/apps: $(ls "${ROOTPATH_TMP}/zpod/apps" | tr '\n' ' ')"
else
	echo "post-image: ZPOD_HOST_APP not set; apps install on first boot"
fi
chown -R 1000:1000 "${ROOTPATH_TMP}/media"   # login user (uid 1000), as on Pi OS
chown 999:999 "${ROOTPATH_TMP}/zpod/appdata"   # zpod (system user)

rm -rf "${GENIMAGE_TMP}"
genimage --rootpath "${ROOTPATH_TMP}" --tmppath "${GENIMAGE_TMP}" \
	--inputpath "${BINARIES_DIR}" --outputpath "${BINARIES_DIR}" \
	--config "${GENIMAGE_CFG}"
