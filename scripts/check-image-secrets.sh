#!/usr/bin/env bash
# Leak check: search a ZPOD .img for the real Wi-Fi SSID and password from a
# LOCAL env file (default config/wifi.local.env, gitignored). Prints match
# COUNTS only, never the strings. Exit 1 if anything matches.
# Searches: the raw image bytes (incl. free blocks), every file of the root
# partition (mounted read-only via offset loop) and of the FAT boot partition
# (copied out with mtools). Needs root (losetup/mount), parted, mtools.
# Usage: sudo scripts/check-image-secrets.sh image.img [wifi.local.env]
set -euo pipefail
IMG="${1:?usage: $0 image.img [wifi.local.env]}"
ENV_FILE="${2:-$(cd "$(dirname "$0")/.." && pwd)/config/wifi.local.env}"
[[ -f "$ENV_FILE" ]] || { echo "error: $ENV_FILE not found" >&2; exit 2; }
getv() { # parse KEY=value without sourcing; strip one pair of quotes
  local v; v=$(grep -m1 "^$1=" "$ENV_FILE" | cut -d= -f2- | tr -d '\r') || true
  if [[ "$v" =~ ^\"(.*)\"$ || "$v" =~ ^\'(.*)\'$ ]]; then v="${BASH_REMATCH[1]}"; fi
  printf '%s' "$v"; }
PASS_S="$(getv WPA_PASSWORD)"; SSID_S="$(getv WPA_SSID)"
[[ -n "$PASS_S" && -n "$SSID_S" ]] || { echo "error: WPA_SSID/WPA_PASSWORD missing in env file" >&2; exit 2; }
read -r BOOT_START ROOT_START ROOT_SIZE < <(parted -m -s "$IMG" unit B print |
  awk -F: '$1=="1"{b=$2} $1=="2"{r=$2; s=$4} END{gsub("B","",b); gsub("B","",r); gsub("B","",s); print b, r, s}')
TMP=$(mktemp -d); MNT="$TMP/root"; BOOTD="$TMP/boot"; mkdir -p "$MNT" "$BOOTD"; LOOP=""
cleanup() { set +e; umount -l "$MNT" 2>/dev/null; [[ -n "$LOOP" ]] && losetup -d "$LOOP"; rm -rf "$TMP"; }
trap cleanup EXIT
MTOOLS_SKIP_CHECK=1 mcopy -s -n -i "${IMG}@@${BOOT_START}" '::*' "$BOOTD/" 2>/dev/null
LOOP=$(losetup -f --show -r --offset "$ROOT_START" --sizelimit "$ROOT_SIZE" "$IMG")
mount -o ro,noload -t ext4 "$LOOP" "$MNT"
total=0
for which in password SSID; do
  if [[ $which == password ]]; then S="$PASS_S"; else S="$SSID_S"; fi
  raw=$(LC_ALL=C grep -caF -- "$S" "$IMG" || true)
  rootf=$(LC_ALL=C grep -rlaF -D skip -s -- "$S" "$MNT" | wc -l | tr -d ' ')
  bootf=$(LC_ALL=C grep -rlaF -s -- "$S" "$BOOTD" | wc -l | tr -d ' ')
  echo "$which: raw_image_matches=$raw rootfs_files=$rootf bootfs_files=$bootf"
  total=$((total + raw + rootf + bootf))
done
unset PASS_S SSID_S S
echo "bootfs_files_checked=$(find "$BOOTD" -type f | wc -l | tr -d ' ') rootfs_files_checked=$(find "$MNT" -xdev -type f 2>/dev/null | wc -l | tr -d ' ')"
if [[ $total -eq 0 ]]; then echo "SECRETS_CHECK_OK (0 matches)"; else echo "SECRETS_CHECK_FAIL ($total matches)"; exit 1; fi
