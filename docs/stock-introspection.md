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

## Vendor / online display evidence (not live-board verified)

**Source class:** Paul pasted UGEEK ZPOD display facts from online/vendor material (2026-10-07 PT). Treat as **candidate** facts until confirmed from assembly guide, Pirate Audio reference matched to ZPOD, or live GPIO / `/dev` readout on a booted board.

| Fact | Value | Status |
| --- | --- | --- |
| Panel | 1.3-inch IPS color LCD | Online only |
| Resolution | 240×240 | Online only |
| Driver chip | ST7789 | Online only |
| Software path (apps) | Pirate Audio / Pimoroni-style Python ST7789 over SPI | Online only |
| Gaming path | Often fb mirroring (`fbcp-ili9341` / `st7789v` → `/dev/fb1`) | Online only |
| Our OS path | Raspberry Pi OS lite via pi-gen (not RetroPie/Volumio as base); primary UI = music/portable apps + buttons; emulation secondary | Project policy |

**Do not invent** BCM pin numbers for DC / BL / CS (or any other TFT GPIO) until confirmed from assembly guide, Pirate Audio pinout matched to this ZPOD, or live `gpio` / device-tree readout.

**Conflict with earlier tree notes:** `config/zpod.yaml` still records vendor-tree names (`pitft22`, 2.2", 320×240, driver unknown) from howardqiao sources. Those remain unmerged with this ST7789 / 1.3" / 240×240 online claim until hardware introspect closes the gap. Prefer live-board evidence over either source if they disagree.

**Next:** once pins are confirmed, software bring-up can start (Python ST7789 SPI and/or fb path); track under [#6 M4](https://github.com/nymble/zpod/issues/6) and [#12 INTROSPECT](https://github.com/nymble/zpod/issues/12).
