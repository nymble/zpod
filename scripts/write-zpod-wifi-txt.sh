#!/usr/bin/env bash
# Mac helper: after flashing, write zpod-wifi.txt onto the card's boot volume
# from your LOCAL config/wifi.local.env (WPA_SSID / WPA_PASSWORD / WPA_COUNTRY).
# The ZPOD imports it on first boot and deletes it. Prints no secrets.
# Usage: scripts/write-zpod-wifi-txt.sh [/Volumes/bootfs]
set -euo pipefail
VOL="${1:-/Volumes/bootfs}"
ENV_FILE="$(cd "$(dirname "$0")/.." && pwd)/config/wifi.local.env"
[[ -d "$VOL" ]] || { echo "error: $VOL not mounted (re-insert the card)" >&2; exit 1; }
[[ -f "$ENV_FILE" ]] || { echo "error: $ENV_FILE not found" >&2; exit 1; }
getv() { local v; v=$(grep -m1 "^$1=" "$ENV_FILE" | cut -d= -f2- | tr -d '\r') || true
  if [[ "$v" =~ ^\"(.*)\"$ || "$v" =~ ^\'(.*)\'$ ]]; then v="${BASH_REMATCH[1]}"; fi
  printf '%s' "$v"; }
umask 077
{ printf 'SSID=%s\n' "$(getv WPA_SSID)"; printf 'PSK=%s\n' "$(getv WPA_PASSWORD)"
  c="$(getv WPA_COUNTRY)"; [[ -n "$c" ]] && printf 'COUNTRY=%s\n' "$c"; } > "$VOL/zpod-wifi.txt"
sync
echo "wrote $VOL/zpod-wifi.txt (imported and deleted by the ZPOD on first boot)"
