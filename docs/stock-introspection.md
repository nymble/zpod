# Stock / working-image introspection

Status: **empty — waiting on a working card or image Paul can share.**

Purpose: record what the current distribution actually contains so new requirements are evidence-based, without forcing the first new build to clone 2017 feature-for-feature.

## Capture checklist (run on a booted ZPOD or from a mounted image)

- [ ] `uname -a`, `/etc/os-release`, kernel package version
- [ ] `/boot/config.txt` and `/boot/cmdline.txt` (full)
- [ ] `ls /boot/overlays | sort` and note `aoide-zpod-dac`, `pitft22`, `gpio-ir`
- [ ] `lsmod`, `aplay -l`, `i2cdetect -y 1`
- [ ] `dpkg -l` (or `rpm -qa`) saved as an artifact
- [ ] systemd units / init scripts that start the UI, player, RetroPie, or button daemon
- [ ] Button daemon config path and GPIO map file, if any
- [ ] Network: `iw list`, `hciconfig` / `bluetoothctl show`, USB Wi-Fi dongle VID:PID if present
- [ ] Partition layout (`lsblk -f`) and media mount points

## Where results go

- Attach logs to issue **[INTROSPECT]**.
- Summarize durable facts into `config/hardware.yaml` and this file via PR.
- Turn each unexpected package or missing overlay into a linked sub-issue; do not silently expand scope.
