#!/usr/bin/env bash
# Leak check for a Buildroot ZPOD image (squashfs roots, FAT boot, ext4 /data)
# and an A/B update bundle (.zab). Prints counts only, never the matches.
# Linux, as root (the zpod-br build container): needs sfdisk, mtools,
# unsquashfs, debugfs, tar.
#   check-image-secrets-br.sh <sdcard.img> [bundle.zab] [wifi.local.env]
# Looks for:
#  1. the real Wi-Fi SSID and password from a LOCAL env file (WPA_SSID /
#     WPA_PASSWORD, default config/wifi.local.env, gitignored), if one
#     exists. Without it, only the known home SSID (ZPOD_CHECK_SSID, case-
#     insensitive) is searched and the report says the password was NOT
#     checked;
#  2. credential-shaped content: psk= / password= values that are not
#     placeholders, private keys, Wi-Fi drop-in and login files that only a
#     user should put on a card (zpod-wifi.txt, userconf.txt, zpod-ssh.pub),
#     non-empty authorized_keys, NetworkManager / wpa_supplicant networks;
# in: the raw image bytes; every file of both squashfs roots (p2, p3); every
# file of the FAT boot partition (p1) and of /data (p4); the raw bundle and
# every file in it, including its squashfs. Exit 1 if anything matches.
set -uo pipefail
IMG=${1:?usage: $0 sdcard.img [bundle.zab] [wifi.local.env]}
ZAB=${2:-}
ENV_FILE=${3:-$(cd "$(dirname "$0")/.." && pwd)/config/wifi.local.env}
SSID_HINT=${ZPOD_CHECK_SSID:-}
W=$(mktemp -d); trap 'rm -rf "$W"' EXIT
total=0
note() { printf '%-58s %s\n' "$1" "$2"; }
hit() { note "$1" "$2"; [ "$2" = 0 ] || total=$((total + $2)); }
getv() { local v; v=$(grep -m1 "^$1=" "$ENV_FILE" | cut -d= -f2- | tr -d '\r') || true
	if [[ "$v" =~ ^\"(.*)\"$ || "$v" =~ ^\'(.*)\'$ ]]; then v="${BASH_REMATCH[1]}"; fi; printf '%s' "$v"; }
PASS_S= SSID_S=
if [ -f "$ENV_FILE" ]; then PASS_S=$(getv WPA_PASSWORD); SSID_S=$(getv WPA_SSID); fi

# --- unpack
read -r -a P <<< "$(sfdisk -d "$IMG" | awk -F'[=,]' '/start=/ {gsub(/ /,""); printf "%s:%s ", $2, $4}')"
mkdir -p "$W/fat" "$W/rootA" "$W/rootB" "$W/data"
off() { echo $(( ${P[$1]%%:*} * 512 )); }
len() { echo $(( ${P[$1]##*:} * 512 )); }
MTOOLS_SKIP_CHECK=1 mcopy -s -n -i "${IMG}@@$(off 0)" '::*' "$W/fat/" 2>/dev/null
dd if="$IMG" of="$W/p2" bs=512 skip=${P[1]%%:*} count=${P[1]##*:} status=none
dd if="$IMG" of="$W/p3" bs=512 skip=${P[2]%%:*} count=${P[2]##*:} status=none
dd if="$IMG" of="$W/p4" bs=512 skip=${P[3]%%:*} count=${P[3]##*:} status=none
unsquashfs -q -n -d "$W/rootA/x" "$W/p2" >/dev/null 2>&1
unsquashfs -q -n -d "$W/rootB/x" "$W/p3" >/dev/null 2>&1
debugfs -R "rdump / $W/data" "$W/p4" >/dev/null 2>&1
SETS="fat rootA rootB data"
if [ -n "$ZAB" ]; then
	mkdir -p "$W/zab" "$W/zabroot"
	tar -xf "$ZAB" -C "$W/zab"
	unsquashfs -q -n -d "$W/zabroot/x" "$W/zab/rootfs.squashfs" >/dev/null 2>&1
	SETS="$SETS zab zabroot"
fi
for s in $SETS; do note "files in $s" "$(find "$W/$s" -type f | wc -l)"; done
[ "$(find "$W/rootA" -type f | wc -l)" -gt 1000 ] || { echo "error: root A did not unpack"; exit 2; }

# --- 1. known strings
count_str() { # label string [-i]
	local n raw
	raw=$(LC_ALL=C grep -ca $3 -F -- "$2" "$IMG" || true)
	n=$raw
	for s in $SETS; do n=$(( n + $(LC_ALL=C grep -rlaF $3 -- "$2" "$W/$s" 2>/dev/null | wc -l) )); done
	[ -n "$ZAB" ] && n=$(( n + $(LC_ALL=C grep -ca $3 -F -- "$2" "$ZAB" || true) ))
	hit "$1" "$n"
}
if [ -n "$PASS_S" ] && [ -n "$SSID_S" ]; then
	count_str "real Wi-Fi password (env file)" "$PASS_S"
	count_str "real Wi-Fi SSID (env file)" "$SSID_S"
	PW_CHECKED=yes
else
	PW_CHECKED=no
	note "real Wi-Fi password" "NOT CHECKED (no $ENV_FILE)"
fi
[ -n "$SSID_HINT" ] && count_str "home SSID from ZPOD_CHECK_SSID (case-insensitive)" "$SSID_HINT" -i

# --- 2. credential-shaped content
n=0
for s in $SETS; do
	n=$(( n + $(LC_ALL=C grep -rlaE '^[[:space:]]*(psk|password|wpa_passphrase|wep_key0)[[:space:]]*=[[:space:]]*"?[^"<$[:space:]][^"]{3,}' "$W/$s" 2>/dev/null \
		| xargs -r grep -LaE '(<PW>|<PSK>|example|placeholder)' 2>/dev/null | wc -l) ))
done
hit "files with a psk= / password= value (not placeholders)" "$n"
n=0; for s in $SETS; do n=$(( n + $(LC_ALL=C grep -rlIE -- '-----BEGIN ([A-Z]+ )?PRIVATE KEY-----' "$W/$s" 2>/dev/null | wc -l) )); done
hit "private keys (PEM in text files; programs only hold the format)" "$n"
n=$(find "$W/fat" "$W/data" -maxdepth 3 \( -iname 'zpod-wifi.txt' -o -iname 'userconf.txt' -o -iname userconf -o -iname 'zpod-ssh.pub' -o -iname 'wpa_supplicant.conf' \) | wc -l)
hit "user drop-in files on FAT or /data" "$n"
n=0; for s in $SETS; do n=$(( n + $(find "$W/$s" -name authorized_keys -size +0 | wc -l) )); done
hit "non-empty authorized_keys" "$n"
n=0; for s in $SETS; do n=$(( n + $(find "$W/$s" -path '*system-connections/*' -type f | wc -l) )); done
hit "NetworkManager connection files" "$n"
n=0; for s in $SETS; do n=$(( n + $(LC_ALL=C grep -rlaE '^[[:space:]]*ssid="[^"<]' "$W/$s" 2>/dev/null | wc -l) )); done
hit "wpa_supplicant network blocks with an SSID" "$n"
n=0; for s in $SETS; do n=$(( n + $(LC_ALL=C grep -rlaE '/Users/(paul|leslie)|764245\+nymble@|elbmyn' "$W/$s" 2>/dev/null | wc -l) )); done
note "Mac home paths / account names (informational)" "$n"

echo "password checked against the real value: $PW_CHECKED"
if [ "$total" -eq 0 ]; then echo "RESULT: 0 matches"; exit 0; fi
echo "RESULT: $total matches"; exit 1
