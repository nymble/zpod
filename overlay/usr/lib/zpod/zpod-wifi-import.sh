#!/bin/bash
# ZPOD first-boot Wi-Fi import (M3.1+). Images never contain Wi-Fi credentials.
#
# If the FAT boot partition has
#   zpod-wifi.txt        lines: SSID=<name>  PSK=<password>  [COUNTRY=US] [HIDDEN=yes]
#                        (empty or missing PSK = open network; '#' = comment)
#   *.nmconnection       ready-made NetworkManager keyfile(s)
# this creates/installs the NetworkManager profile(s) under
# /etc/NetworkManager/system-connections (root:root 0600, Wi-Fi power save off)
# and then overwrites and deletes the source file from the boot partition.
# Never prints the SSID or the password.
set -euo pipefail
BOOT="${ZPOD_BOOT_DIR:-/boot/firmware}"
CONN_DIR="${ZPOD_CONN_DIR:-/etc/NetworkManager/system-connections}"
TXT="$BOOT/zpod-wifi.txt"
log() { echo "zpod-wifi-import: $*"; }
# M6: files that arrive after the first boot are reported on the launcher's
# first screen (one line each in /run/zpod/boot-notice; no SSID, no password).
BOOTED_ONCE="${ZPOD_BOOTED_ONCE:-/var/lib/zpod/booted-once}"
NOTICE="${ZPOD_NOTICE:-/run/zpod/boot-notice}"
notice() { [ -e "$BOOTED_ONCE" ] || return 0; mkdir -p "$(dirname "$NOTICE")"; printf '%s\n' "$*" >> "$NOTICE"; }
umask 077

# Overwrite with zeros, then unlink, then flush. Best effort: SD cards remap
# blocks (wear levelling), so treat an imported card as having held the key.
wipe() {
  local f="$1"
  [ -e "$f" ] || return 0
  shred -z -n 1 -u "$f" 2>/dev/null || {
    dd if=/dev/zero of="$f" bs=4096 count=$(( ($(stat -c %s "$f") + 4095) / 4096 )) conv=notrunc,fsync 2>/dev/null || true
    rm -f "$f"
  }
  sync
}

# GKeyFile string escaping (NM keyfile): backslash, tab, leading space.
kf_escape() {
  local s="${1//\\/\\\\}"
  s="${s//$'\t'/\\t}"
  [[ "$s" == " "* ]] && s="\\s${s:1}"
  printf '%s' "$s"
}

install -d -m 700 -o root -g root "$CONN_DIR"
imported=0

if [ -f "$TXT" ]; then
  ssid="" psk="" country="" hidden="false" have_psk=0
  first=1
  while IFS= read -r line || [ -n "$line" ]; do
    line="${line%$'\r'}"
    if [ "$first" = 1 ]; then line="${line#$'\xef\xbb\xbf'}"; first=0; fi
    case "$line" in
      ''|'#'*) ;;
      SSID=*) ssid="${line#SSID=}" ;;
      PSK=*|PASSWORD=*) psk="${line#*=}"; have_psk=1 ;;
      COUNTRY=*) country="${line#COUNTRY=}" ;;
      HIDDEN=*) case "${line#HIDDEN=}" in [yY]*|1|true) hidden=true ;; esac ;;
      *) log "ignoring unknown line in zpod-wifi.txt" ;;
    esac
  done < "$TXT"
  ok=1
  if [ -z "$ssid" ] || [ "$(printf '%s' "$ssid" | wc -c)" -gt 32 ]; then
    log "zpod-wifi.txt: SSID missing or longer than 32 bytes; not imported"; ok=0
  fi
  if [ "$have_psk" = 1 ] && [ -n "$psk" ]; then
    n=$(printf '%s' "$psk" | wc -c)
    if ! { [ "$n" -ge 8 ] && [ "$n" -le 63 ]; } && ! [[ "$psk" =~ ^[0-9A-Fa-f]{64}$ ]]; then
      log "zpod-wifi.txt: PSK must be 8-63 characters (or 64 hex digits); not imported"; ok=0
    fi
  fi
  if [ "$ok" = 1 ]; then
    tmp="$(mktemp "$CONN_DIR/.zpod-wifi.XXXXXX")"
    {
      printf '[connection]\nid=zpod-wifi\nuuid=%s\ntype=wifi\nautoconnect=true\n\n' "$(cat /proc/sys/kernel/random/uuid)"
      printf '[wifi]\nmode=infrastructure\nssid=%s\nhidden=%s\n' "$(kf_escape "$ssid")" "$hidden"
      printf '# 2 = power save off (brcmfmac dropouts inside the metal case)\npowersave=2\n\n'
      if [ -n "$psk" ]; then
        printf '[wifi-security]\nkey-mgmt=wpa-psk\npsk=%s\n\n' "$(kf_escape "$psk")"
      fi
      printf '[ipv4]\nmethod=auto\n\n[ipv6]\naddr-gen-mode=default\nmethod=auto\n\n[proxy]\n'
    } > "$tmp"
    chown root:root "$tmp"; chmod 600 "$tmp"
    mv -f "$tmp" "$CONN_DIR/zpod-wifi.nmconnection"
    imported=$((imported + 1))
    log "imported zpod-wifi.txt as NetworkManager profile 'zpod-wifi'"
    notice "zpod-wifi.txt: Wi-Fi network added"
    if [[ "$country" =~ ^[A-Z]{2}$ ]] && command -v raspi-config >/dev/null && [ -z "${ZPOD_NO_COUNTRY:-}" ]; then
      raspi-config nonint do_wifi_country "$country" >/dev/null 2>&1 || log "could not set Wi-Fi country"
    fi
  fi
  [ "$ok" = 1 ] || notice "zpod-wifi.txt: not valid, not imported (deleted)"
  unset ssid psk
  wipe "$TXT"
fi

shopt -s nullglob
for f in "$BOOT"/*.nmconnection; do
  base="$(basename "$f")"
  tmp="$(mktemp "$CONN_DIR/.zpod-import.XXXXXX")"
  tr -d '\r' < "$f" > "$tmp"
  if grep -qE '^type=(wifi|802-11-wireless)$' "$tmp" && ! grep -q '^powersave=' "$tmp"; then
    sed -i '/^\[wifi\]$/a powersave=2' "$tmp"
  fi
  chown root:root "$tmp"; chmod 600 "$tmp"
  mv -f "$tmp" "$CONN_DIR/$base"
  imported=$((imported + 1))
  log "installed NetworkManager profile from boot partition ($base)"
  notice "$base: network profile installed"
  wipe "$f"
done

# If NetworkManager is already running (manual re-run), make it re-read profiles.
if [ "$imported" -gt 0 ] && [ -d /run/systemd/system ] && systemctl -q is-active NetworkManager.service 2>/dev/null; then
  nmcli connection reload || true
fi
log "done ($imported profile(s))"
