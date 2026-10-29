#!/usr/bin/env bash
# Export a pi-gen stage2 rootfs to a Raspberry Pi SD .img using losetup --offset.
# Use when the host kernel does not create /dev/loopNpM partition nodes
# (common in restricted containers). Does not flash a card.
#
# Host quirks handled here:
# - du without -x: overlayfs lower/upper can disagree on st_dev so -x under-counts.
# - No kernel vfat module: populate the FAT bootfs with mtools (not mount -t vfat).
# - mtools on /dev/loop fails ioctl geometry / silent copy fails; use file@@offset.
# - mke2fs -d + hardlink staging can fail on overlay; format empty ext4 then rsync.
#
# Finalize (mirrors pi-gen export-image 01/03/04/05 subset — required for a
# bootable image; a raw stage2 dump is NOT bootable):
# - 04-set-partuuid: replace BOOTDEV/ROOTDEV in fstab + cmdline with PARTUUID
# - 01-user-rename: chroot rename-user -f -s → enables userconfig.service
#   (first-boot HDMI password wizard; pi has no baked password)
# - 02-set-sources subset: remove the build-host APT proxy (51cache)
# - never bakes Wi-Fi credentials (refuses if the rootfs has any); Wi-Fi comes
#   from zpod-wifi.txt on the boot partition at first boot (zpod-wifi-import)
# - 03-network: install resolv.conf
# - 05-finalise subset: ld.so.preload, machine-id=uninitialized, clear logs,
#   remove *- backups, mtab symlink. Skips update-initramfs, apt dist-upgrade,
#   fstrim, info/sbom generation.
#
# M5 large-card layout (docs/STORAGE.md):
#   p1 boot  FAT32 512 MiB
#   p2 root  ext4, FIXED size (rootfs + 30 % + 512 MiB, at least 3 GiB); no
#            first-boot root resize (` resize` is removed from cmdline.txt)
#   p3 data  ext4 "zpoddata", 256 MiB in the image, grown to the end of the
#            card at first boot (zpod-data-grow + x-systemd.growfs), max 2 TiB
#            (MBR). Holds /data/media (bind-mounted on /srv/zpod/media) and
#            /data/zpod/appdata. ZPOD_LAYOUT=single keeps the M4 2-partition
#            layout.
set -euo pipefail

LOCK_FILE="${EXPORT_LOCK_FILE:-/workspace/zpod-build-logs/export.lock}"
mkdir -p "$(dirname "$LOCK_FILE")"
exec 9>"$LOCK_FILE"
if ! flock -n 9; then
  echo "error: another export holds $LOCK_FILE" >&2
  exit 1
fi

ROOTFS="${1:-/workspace/zpod-repo/build/pi-gen/work/zpod-pi0w/stage2/rootfs}"
OUT_DIR="${2:-/workspace/zpod-repo/build/pi-gen/deploy}"
IMG_NAME="${3:-zpod-pi0w}"
IMG_DATE="${IMG_DATE:-$(date +%Y-%m-%d)}"
IMG_FILE="${OUT_DIR}/${IMG_DATE}-${IMG_NAME}.img"

# Path to pi-gen export-image artifacts relative to this repo (or override)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PIGEN_EXPORT="${PIGEN_EXPORT_DIR:-$REPO_ROOT/build/pi-gen/export-image}"
RESOLV_SRC="${PIGEN_EXPORT}/03-network/files/resolv.conf"

if [[ ! -d "$ROOTFS/boot/firmware" ]]; then
  echo "error: missing $ROOTFS/boot/firmware" >&2
  exit 1
fi

# M3.1+: never bake Wi-Fi credentials. config/wifi.local.env is NOT read here;
# the image imports zpod-wifi.txt from the boot partition on first boot.
# Refuse to export a rootfs that carries any Wi-Fi profile or PSK.
cred_found=0
for f in "$ROOTFS"/etc/NetworkManager/system-connections/*.nmconnection; do
  [[ -e "$f" ]] || continue
  if sudo grep -qE '^(type=(wifi|802-11-wireless)|psk=|\[wifi-security\])' "$f"; then
    echo "error: Wi-Fi profile in rootfs: ${f#$ROOTFS}" >&2; cred_found=1
  fi
done
for f in "$ROOTFS"/boot/firmware/wpa_supplicant.conf "$ROOTFS"/boot/firmware/zpod-wifi.txt \
         "$ROOTFS"/boot/firmware/*.nmconnection "$ROOTFS"/etc/wpa_supplicant/*.conf; do
  [[ -e "$f" ]] || continue
  if [[ "$f" != *.conf ]] || sudo grep -qE '^[[:space:]]*(psk|wep_key[0-9]|password)=' "$f"; then
    echo "error: Wi-Fi credentials file in rootfs: ${f#$ROOTFS}" >&2; cred_found=1
  fi
done
[[ "$cred_found" -eq 0 ]] || { echo "error: refusing to export an image with Wi-Fi credentials" >&2; exit 1; }
# Let NetworkManager start with WLAN enabled (no stale state from the build).
sudo rm -f "$ROOTFS/var/lib/NetworkManager/NetworkManager.state"

need() { command -v "$1" >/dev/null || { echo "missing $1" >&2; exit 1; }; }
need truncate; need parted; need losetup; need mkdosfs; need mke2fs; need rsync
need bc; need xz; need sha256sum; need mcopy; need mmd
need xxd; need qemu-arm-static; need chroot

mkdir -p "$OUT_DIR"
BOOT_SIZE="$((512 * 1024 * 1024))"
ROOT_SIZE=$(sudo du -s --block-size=1 "$ROOTFS" | awk '{print $1}')
if [[ -z "$ROOT_SIZE" || "$ROOT_SIZE" -lt $((100 * 1024 * 1024)) ]]; then
  echo "error: rootfs size looks wrong: ROOT_SIZE=${ROOT_SIZE:-empty}" >&2
  exit 1
fi
ALIGN="$((8 * 1024 * 1024))"
LAYOUT="${ZPOD_LAYOUT:-data}"
if [[ "$LAYOUT" == data ]]; then
  ROOT_MARGIN="$(echo "($ROOT_SIZE * 0.3 + 512 * 1024 * 1024) / 1" | bc)"
  ROOT_MIN=$((3 * 1024 * 1024 * 1024))
  DATA_PART_SIZE=$((256 * 1024 * 1024))
else
  ROOT_MARGIN="$(echo "($ROOT_SIZE * 0.2 + 200 * 1024 * 1024) / 1" | bc)"
  ROOT_MIN=0
  DATA_PART_SIZE=0
fi
BOOT_PART_START=$ALIGN
BOOT_PART_SIZE=$(((BOOT_SIZE + ALIGN - 1) / ALIGN * ALIGN))
ROOT_PART_START=$((BOOT_PART_START + BOOT_PART_SIZE))
ROOT_PART_SIZE=$(((ROOT_SIZE + ROOT_MARGIN + ALIGN - 1) / ALIGN * ALIGN))
[[ "$ROOT_PART_SIZE" -lt "$ROOT_MIN" ]] && ROOT_PART_SIZE=$ROOT_MIN
DATA_PART_START=$((ROOT_PART_START + ROOT_PART_SIZE))
IMG_SIZE=$((DATA_PART_START + DATA_PART_SIZE))
echo "layout=$LAYOUT boot=${BOOT_PART_SIZE} root=${ROOT_PART_SIZE} data=${DATA_PART_SIZE}"

echo "rootfs_bytes=$ROOT_SIZE margin=$ROOT_MARGIN img_bytes=$IMG_SIZE -> $IMG_FILE"
rm -f "$IMG_FILE" "${IMG_FILE}.xz" "${IMG_FILE}.sha256" "${OUT_DIR}/SHA256SUMS"
truncate -s "$IMG_SIZE" "$IMG_FILE"
sudo parted --script "$IMG_FILE" mklabel msdos
sudo parted --script "$IMG_FILE" unit B mkpart primary fat32 "${BOOT_PART_START}" "$((BOOT_PART_START + BOOT_PART_SIZE - 1))"
sudo parted --script "$IMG_FILE" unit B mkpart primary ext4 "${ROOT_PART_START}" "$((ROOT_PART_START + ROOT_PART_SIZE - 1))"
if [[ "$DATA_PART_SIZE" -gt 0 ]]; then
  sudo parted --script "$IMG_FILE" unit B mkpart primary ext4 "${DATA_PART_START}" "$((DATA_PART_START + DATA_PART_SIZE - 1))"
fi

BOOT_LOOP=""
ROOT_LOOP=""
DATA_LOOP=""
MNT=""
DMNT=""
BINDS_MOUNTED=0
QEMU_ADDED=0
cleanup() {
  set +e
  if [[ "${BINDS_MOUNTED:-0}" -eq 1 && -n "${MNT:-}" ]]; then
    for m in sys proc dev/pts dev; do
      sudo umount -l "$MNT/$m" 2>/dev/null || true
    done
  fi
  if [[ "${QEMU_ADDED:-0}" -eq 1 && -n "${MNT:-}" ]]; then
    sudo rm -f "$MNT/usr/bin/qemu-arm-static"
  fi
  if [[ -n "${DMNT:-}" ]]; then
    sudo umount -l "$DMNT" 2>/dev/null || true
    sudo rm -rf "$DMNT" 2>/dev/null || true
  fi
  if [[ -n "${DATA_LOOP:-}" ]]; then
    sudo losetup -d "$DATA_LOOP" 2>/dev/null || true
  fi
  if [[ -n "${MNT:-}" ]]; then
    sudo umount -l "$MNT" 2>/dev/null || true
    sudo rm -rf "$MNT" 2>/dev/null || true
  fi
  if [[ -n "${BOOT_LOOP:-}" ]]; then
    sudo losetup -d "$BOOT_LOOP" 2>/dev/null || true
  fi
  if [[ -n "${ROOT_LOOP:-}" ]]; then
    sudo losetup -d "$ROOT_LOOP" 2>/dev/null || true
  fi
}
trap cleanup EXIT

BOOT_LOOP=$(sudo losetup -f --show --offset "$BOOT_PART_START" --sizelimit "$BOOT_PART_SIZE" "$IMG_FILE")
ROOT_LOOP=$(sudo losetup -f --show --offset "$ROOT_PART_START" --sizelimit "$ROOT_PART_SIZE" "$IMG_FILE")

ROOT_FEATURES="^huge_file"
if grep -q 64bit /etc/mke2fs.conf 2>/dev/null; then
  ROOT_FEATURES="^64bit,$ROOT_FEATURES"
fi

echo "Formatting boot ($BOOT_LOOP) and root ($ROOT_LOOP)..."
sudo mkdosfs -n bootfs -F 32 "$BOOT_LOOP" >/dev/null
sudo mke2fs -q -t ext4 -L rootfs -O "$ROOT_FEATURES" "$ROOT_LOOP"
if [[ "$DATA_PART_SIZE" -gt 0 ]]; then
  DATA_LOOP=$(sudo losetup -f --show --offset "$DATA_PART_START" --sizelimit "$DATA_PART_SIZE" "$IMG_FILE")
  # 4 KiB blocks, no root reserve, one inode per 64 KiB (music files are
  # large), and resize_inode room for online growth up to 2 TiB.
  sudo mke2fs -q -t ext4 -L zpoddata -O "$ROOT_FEATURES" -b 4096 -m 0 -i 65536 \
    -E resize=$((2 * 1024 * 1024 * 1024 * 1024 / 4096 - 1)) "$DATA_LOOP"
fi

# Detach boot loop before mtools — use image@@offset (loop ioctl geometry fails)
sudo losetup -d "$BOOT_LOOP"
BOOT_LOOP=""
BOOT_MTOOL="${IMG_FILE}@@${BOOT_PART_START}"

MNT=$(mktemp -d /tmp/zpod-mnt.XXXXXX)
echo "Mounting root and rsyncing rootfs..."
sudo mount -t ext4 "$ROOT_LOOP" "$MNT"
sudo mkdir -p "$MNT/boot/firmware"
sudo rsync -aHAX --numeric-ids \
  --exclude='/var/cache/apt/archives/**' \
  --exclude='/boot/firmware/**' \
  "$ROOTFS"/ "$MNT"/
sudo mkdir -p "$MNT/boot/firmware" "$MNT/var/cache/apt/archives"
if [[ -d "$ROOTFS/var/cache/apt/archives/partial" ]]; then
  sudo mkdir -p "$MNT/var/cache/apt/archives/partial"
fi

# --- Finalize on mounted root (pi-gen export-image 01/03/04/05 subset) ---

# 04-set-partuuid: fstab
IMGID="$(dd if="$IMG_FILE" skip=440 bs=1 count=4 2>/dev/null | xxd -e | cut -f2 -d' ')"
echo "IMGID=$IMGID -> PARTUUID=${IMGID}-01 (boot) / ${IMGID}-02 (root)"
sudo sed -i "s/BOOTDEV/PARTUUID=${IMGID}-01/; s/ROOTDEV/PARTUUID=${IMGID}-02/" "$MNT/etc/fstab"

# M5: partition 3 = /data. Move the media tree there; root keeps empty
# mount points. fstab: /data (grown online at first boot) + bind mount.
if [[ "$DATA_PART_SIZE" -gt 0 ]]; then
  echo "PARTUUID=${IMGID}-03 -> /data"
  DMNT=$(mktemp -d /tmp/zpod-data.XXXXXX)
  sudo mount -t ext4 "$DATA_LOOP" "$DMNT"
  sudo rmdir "$DMNT/lost+found" 2>/dev/null || true
  sudo install -d -m 0755 "$DMNT/media" "$DMNT/zpod"
  if [[ -d "$MNT/srv/zpod/media" ]]; then
    sudo rsync -aHAX --numeric-ids "$MNT/srv/zpod/media/" "$DMNT/media/"
    sudo find "$MNT/srv/zpod/media" -mindepth 1 -delete
  fi
  sudo install -d -m 0755 "$MNT/srv/zpod/media" "$MNT/data"
  sudo chown --reference="$DMNT/media/music" "$DMNT/media" 2>/dev/null || true
  ZUID=$(awk -F: '$1=="zpod"{print $3}' "$MNT/etc/passwd")
  ZGID=$(awk -F: '$1=="zpod"{print $4}' "$MNT/etc/passwd")
  sudo install -d -m 0755 -o "${ZUID:-0}" -g "${ZGID:-0}" "$DMNT/zpod/appdata"
  sudo install -d -m 0700 "$DMNT/lost+found" 2>/dev/null || true
  echo "data partition:"; sudo find "$DMNT" -maxdepth 3 -printf '  %M %U:%G %p\n' | sed "s|$DMNT|/data|"
  sudo umount "$DMNT"; sudo rm -rf "$DMNT"; DMNT=""
  sudo losetup -d "$DATA_LOOP"; DATA_LOOP=""
  sudo sed -i '/[[:space:]]\/data[[:space:]]/d; /[[:space:]]\/srv\/zpod\/media[[:space:]]/d' "$MNT/etc/fstab"
  {
    echo "PARTUUID=${IMGID}-03  /data           ext4    defaults,noatime,nofail,x-systemd.growfs,x-systemd.device-timeout=20s  0       2"
    echo "/data/media           /srv/zpod/media none    bind,nofail,x-systemd.requires-mounts-for=/data  0       0"
  } | sudo tee -a "$MNT/etc/fstab" >/dev/null
  echo "fstab:"; sed 's/^/  /' "$MNT/etc/fstab"
fi

# 01-user-rename: enable first-boot userconfig wizard
echo "Running rename-user -f -s in chroot (enables userconfig.service)..."
for m in dev dev/pts proc sys; do
  sudo mount --bind "/$m" "$MNT/$m"
done
BINDS_MOUNTED=1
if [[ ! -e "$MNT/usr/bin/qemu-arm-static" ]]; then
  sudo cp /usr/bin/qemu-arm-static "$MNT/usr/bin/"
  QEMU_ADDED=1
fi
sudo chroot "$MNT" env SUDO_USER=pi /bin/bash /usr/bin/rename-user -f -s
for m in sys proc dev/pts dev; do
  sudo umount -l "$MNT/$m" 2>/dev/null || true
done
BINDS_MOUNTED=0
if [[ "$QEMU_ADDED" -eq 1 ]]; then
  sudo rm -f "$MNT/usr/bin/qemu-arm-static"
  QEMU_ADDED=0
fi

# M4.1: build identity: add the final image name; derive ZPOD_VERSION from it
# when the build did not set one (e.g. zpod-pi0w-m4.1-candidate -> m4.1-candidate).
REL="$MNT/etc/zpod-release"
[[ -f "$REL" ]] || echo "# ZPOD build identity" | sudo tee "$REL" >/dev/null
sudo sed -i '/^ZPOD_IMAGE=/d' "$REL"
echo "ZPOD_IMAGE=${IMG_DATE}-${IMG_NAME}" | sudo tee -a "$REL" >/dev/null
if ! grep -q '^ZPOD_VERSION=.' "$REL"; then
  sudo sed -i '/^ZPOD_VERSION=/d' "$REL"
  echo "ZPOD_VERSION=${IMG_NAME#zpod-pi0w-}" | sudo tee -a "$REL" >/dev/null
fi
sudo chmod 644 "$REL"
echo "zpod-release:"; sed 's/^/  /' "$REL"

# 03-network
if [[ -f "$RESOLV_SRC" ]]; then
  sudo install -m 644 "$RESOLV_SRC" "$MNT/etc/resolv.conf"
else
  echo "nameserver 8.8.8.8" | sudo tee "$MNT/etc/resolv.conf" >/dev/null
  sudo chmod 644 "$MNT/etc/resolv.conf"
  echo "warn: $RESOLV_SRC missing; wrote fallback 8.8.8.8" >&2
fi

# 05-finalise subset (skip update-initramfs, apt, fstrim, info/sbom)
if [[ -e "$MNT/etc/ld.so.preload.disabled" ]]; then
  sudo mv "$MNT/etc/ld.so.preload.disabled" "$MNT/etc/ld.so.preload"
fi
# 02-set-sources: never ship a build-host APT proxy (pi-gen APT_PROXY)
sudo rm -f "$MNT/etc/apt/apt.conf.d/51cache"
sudo rm -f "$MNT/var/lib/dbus/machine-id"
echo uninitialized | sudo tee "$MNT/etc/machine-id" >/dev/null
sudo find "$MNT/var/log" -type f -exec truncate -s0 {} \;
sudo rm -f "$MNT"/etc/{passwd,group,shadow,gshadow,subuid,subgid}-
sudo ln -nsf /proc/mounts "$MNT/etc/mtab"

# Optional: stage rpi-issue for boot FAT copy below
RPI_ISSUE=""
if [[ -f "$MNT/etc/rpi-issue" ]]; then
  RPI_ISSUE=$(mktemp)
  sudo cp "$MNT/etc/rpi-issue" "$RPI_ISSUE"
  sudo chmod 644 "$RPI_ISSUE"
fi

echo "Populating boot FAT via mtools file@@offset..."
fail=0
while IFS= read -r -d '' d; do
  rel="${d#./}"
  [[ -z "$rel" || "$rel" == "." ]] && continue
  if ! sudo env MTOOLS_SKIP_CHECK=1 mmd -D s -i "$BOOT_MTOOL" "::$rel" 2>/dev/null; then
    sudo env MTOOLS_SKIP_CHECK=1 mmd -i "$BOOT_MTOOL" "::$rel" || true
  fi
done < <(cd "$ROOTFS/boot/firmware" && find . -type d -print0 | sort -z)

copied=0
while IFS= read -r -d '' f; do
  rel="${f#./}"
  if ! sudo env MTOOLS_SKIP_CHECK=1 mcopy -o -i "$BOOT_MTOOL" "$ROOTFS/boot/firmware/$rel" "::$rel"; then
    echo "error: mcopy failed for $rel" >&2
    fail=1
    break
  fi
  copied=$((copied + 1))
done < <(cd "$ROOTFS/boot/firmware" && find . -type f -print0)
echo "mtools copied $copied boot files"

if [[ "$fail" -ne 0 ]]; then
  exit 1
fi

# Overwrite cmdline.txt with PARTUUID-patched version (do NOT leave ROOTDEV)
CMDLINE_TMP=$(mktemp)
sed "s/ROOTDEV/PARTUUID=${IMGID}-02/" "$ROOTFS/boot/firmware/cmdline.txt" > "$CMDLINE_TMP"
if [[ "$DATA_PART_SIZE" -gt 0 ]]; then
  # M5: root has a fixed size; the initramfs must not grow p2 (p3 follows it)
  sed -i 's/ resize\b//; s/^resize //' "$CMDLINE_TMP"
  if grep -qw resize "$CMDLINE_TMP"; then echo "error: cmdline.txt still has resize" >&2; exit 1; fi
fi
sudo env MTOOLS_SKIP_CHECK=1 mcopy -o -i "$BOOT_MTOOL" "$CMDLINE_TMP" ::cmdline.txt
rm -f "$CMDLINE_TMP"
echo "boot FAT cmdline.txt patched with PARTUUID=${IMGID}-02"

if [[ -n "$RPI_ISSUE" && -f "$RPI_ISSUE" ]]; then
  sudo env MTOOLS_SKIP_CHECK=1 mcopy -o -i "$BOOT_MTOOL" "$RPI_ISSUE" ::issue.txt || true
  rm -f "$RPI_ISSUE"
fi

if ! sudo env MTOOLS_SKIP_CHECK=1 mdir -i "$BOOT_MTOOL" :: | grep -qiE 'cmdline(\.txt|[[:space:]]+txt)'; then
  echo "error: cmdline.txt missing from boot FAT after mtools copy" >&2
  sudo env MTOOLS_SKIP_CHECK=1 mdir -i "$BOOT_MTOOL" :: >&2 || true
  exit 1
fi
# Sanity: cmdline must not still say ROOTDEV
VERIFY_CMDLINE=$(mktemp)
sudo env MTOOLS_SKIP_CHECK=1 mcopy -n -i "$BOOT_MTOOL" ::cmdline.txt "$VERIFY_CMDLINE"
if grep -q ROOTDEV "$VERIFY_CMDLINE"; then
  echo "error: cmdline.txt still contains ROOTDEV after patch" >&2
  cat "$VERIFY_CMDLINE" >&2
  rm -f "$VERIFY_CMDLINE"
  exit 1
fi
echo "boot FAT cmdline: $(tr -d '\n' < "$VERIFY_CMDLINE")"
rm -f "$VERIFY_CMDLINE"

sudo umount "$MNT"
sudo losetup -d "$ROOT_LOOP"
ROOT_LOOP=""
trap - EXIT
sudo rm -rf "$MNT"
MNT=""

echo "Compressing..."
xz -T0 -9 -f -k "$IMG_FILE"
XZ="${IMG_FILE}.xz"
(
  cd "$OUT_DIR"
  # SHA256SUMS lists only the published .img.xz so `sha256sum -c SHA256SUMS`
  # passes in a download folder. The uncompressed .img hash goes to its own
  # file (and into the release notes as text).
  sha256sum "$(basename "$XZ")" | tee SHA256SUMS
  sha256sum "$(basename "$IMG_FILE")" | tee "$(basename "$IMG_FILE").sha256"
)
xz -l "$XZ"
ls -lh "$IMG_FILE" "$XZ"
echo "EXPORT_OK $XZ"
