#!/usr/bin/env bash
# Write NetworkManager preconfigured Wi-Fi into a stage2 rootfs.
# Reads WPA_SSID / WPA_PASSWORD / WPA_COUNTRY from env or wifi.local.env.
# Does not flash a card. Does not print the password.
set -euo pipefail
ROOTFS="${1:?usage: $0 /path/to/rootfs}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOCAL_ENV="${REPO_ROOT}/config/wifi.local.env"
if [[ -f "$LOCAL_ENV" ]]; then
  # shellcheck disable=SC1090
  set -a; source "$LOCAL_ENV"; set +a
fi
: "${WPA_SSID:?WPA_SSID required}"
: "${WPA_PASSWORD:?WPA_PASSWORD required}"
WPA_COUNTRY="${WPA_COUNTRY:-US}"

CONN_DIR="${ROOTFS}/etc/NetworkManager/system-connections"
sudo mkdir -p "$CONN_DIR"
# Stable UUID so rebuilds are deterministic for this SSID
UUID="$(printf '%s' "zpod-${WPA_SSID}" | sha256sum | awk '{print substr($1,1,8)"-"substr($1,9,4)"-"substr($1,13,4)"-"substr($1,17,4)"-"substr($1,21,12)}')"
TMP="$(mktemp)"
cat > "$TMP" <<CONN
[connection]
id=preconfigured
uuid=${UUID}
type=wifi
autoconnect=true

[wifi]
mode=infrastructure
ssid=${WPA_SSID}
hidden=false
# 2 = power save off (brcmfmac dropouts inside the metal case)
powersave=2

[wifi-security]
key-mgmt=wpa-psk
psk=${WPA_PASSWORD}

[ipv4]
method=auto

[ipv6]
addr-gen-mode=default
method=auto

[proxy]
CONN
sudo install -m 600 -o root -g root "$TMP" "${CONN_DIR}/preconfigured.nmconnection"
rm -f "$TMP"

# Ensure WLAN not disabled; set regulatory domain via raspi-config if chrootable
if [[ -f "${ROOTFS}/var/lib/NetworkManager/NetworkManager.state" ]]; then
  sudo rm -f "${ROOTFS}/var/lib/NetworkManager/NetworkManager.state"
fi

# Drop classic boot-partition wpa_supplicant.conf into firmware dir so first boot
# also works if something consumes it from /boot/firmware.
BOOTFW="${ROOTFS}/boot/firmware"
if [[ -d "$BOOTFW" ]]; then
  TMP2="$(mktemp)"
  cat > "$TMP2" <<WPA
ctrl_interface=DIR=/var/run/wpa_supplicant GROUP=netdev
update_config=1
country=${WPA_COUNTRY}

network={
	ssid="${WPA_SSID}"
	psk="${WPA_PASSWORD}"
	key_mgmt=WPA-PSK
}
WPA
  sudo install -m 600 -o root -g root "$TMP2" "${BOOTFW}/wpa_supplicant.conf"
  rm -f "$TMP2"
fi

# Best-effort wifi country inside rootfs without full chroot if possible
if [[ -x "${ROOTFS}/usr/bin/raspi-config" ]] || [[ -x "${ROOTFS}/usr/sbin/raspi-config" ]]; then
  if command -v qemu-arm-static >/dev/null && [[ -d /proc/sys/fs/binfmt_misc ]]; then
    sudo mkdir -p "${ROOTFS}/usr/bin"
    if [[ ! -e "${ROOTFS}/usr/bin/qemu-arm-static" ]]; then
      sudo cp /usr/bin/qemu-arm-static "${ROOTFS}/usr/bin/qemu-arm-static"
      QEMU_ADDED=1
    fi
    sudo mount --bind /dev "${ROOTFS}/dev" 2>/dev/null || true
    sudo mount --bind /proc "${ROOTFS}/proc" 2>/dev/null || true
    sudo mount --bind /sys "${ROOTFS}/sys" 2>/dev/null || true
    sudo chroot "$ROOTFS" raspi-config nonint do_wifi_country "${WPA_COUNTRY}" || true
    sudo umount "${ROOTFS}/dev" 2>/dev/null || true
    sudo umount "${ROOTFS}/proc" 2>/dev/null || true
    sudo umount "${ROOTFS}/sys" 2>/dev/null || true
    if [[ "${QEMU_ADDED:-0}" == "1" ]]; then
      sudo rm -f "${ROOTFS}/usr/bin/qemu-arm-static"
    fi
  fi
fi

echo "wifi_injected ssid=${WPA_SSID} country=${WPA_COUNTRY} rootfs=${ROOTFS}"
