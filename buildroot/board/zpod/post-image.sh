#!/bin/bash
# ZPOD post-image: config.txt/cmdline.txt, seed the data partition with the
# bundled apps, and assemble sdcard.img (boot / rootA / rootB / data).
set -e
BOARD_DIR="$(dirname "$0")"
GENIMAGE_CFG="${BINARIES_DIR}/genimage-zpod.cfg"
GENIMAGE_TMP="${BUILD_DIR}/genimage.tmp"

install -m 0644 "${BOARD_DIR}/config.txt"  "${BINARIES_DIR}/rpi-firmware/config.txt"
install -m 0644 "${BOARD_DIR}/cmdline.txt" "${BINARIES_DIR}/rpi-firmware/cmdline.txt"

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
