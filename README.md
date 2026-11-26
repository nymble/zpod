# zpod

Firmware and image pipeline for the **uGeek ZPOD** (Raspberry Pi Zero / Zero W portable player).

Public home: this repository under [nymble](https://github.com/nymble).

## Status

Published images ([Releases](https://github.com/nymble/zpod/releases)):

| Release | What |
| --- | --- |
| `v0.4.1-m3` | **Current.** M3 (shutdown screen, battery, USB reconnect, vendor button labels, faster boot) with **no embedded Wi-Fi credentials**: add Wi-Fi after flashing with `zpod-wifi.txt` on the boot volume ([docs/BOOT-WIFI.md](./docs/BOOT-WIFI.md)); test with [docs/TEST-M3.md](./docs/TEST-M3.md) |
| `v0.4.0-m3` | *Image withdrawn 2026-10-18 (embedded Wi-Fi credentials); use `v0.4.1-m3`.* **M3**: M2 hardware fixes — USB DHCP on macOS (`zpod-usb-kick`), battery (`i2c-dev`), "ZPOD off / Safe to power off" shutdown screen, vendor button labels, faster boot — test with [docs/TEST-M3.md](./docs/TEST-M3.md) |
| `v0.3.0-m2` | *Image withdrawn 2026-10-18 (embedded Wi-Fi credentials).* **M2**: LCD status/menu UI (`zpod-ui`), buttons → keyboard (`zpod-buttons`), Aoide PCM5122 DAC via DKMS, USB-cable Ethernet to a Mac, Wi-Fi power save off — test with [docs/TEST-M2.md](./docs/TEST-M2.md) |
| `v0.2.0-m1.1-wifi` | *Image withdrawn 2026-10-18 (embedded Wi-Fi credentials).* M1.1: lite trixie armhf + Wi-Fi stack |

Requirements and staged plan:

- [REQUIREMENTS.md](./REQUIREMENTS.md)
- [PLAN.md](./PLAN.md)
- [docs/stock-introspection.md](./docs/stock-introspection.md)

## What we are building

A new armhf image family (pi-gen), not a byte-for-byte rebuild of the 2017 SourceForge images. Display, buttons, DAC and fuel gauge use only the pin maps confirmed on hardware on 2026-10-07 ([stock-introspection](./docs/stock-introspection.md)).

Repo layout: `build/` (pi-gen tweaks + M2 substep), `overlay/` (files installed into the image: `zpod-ui`, `zpod-buttons`, configs), `drivers/aoide-zpod-dac/` (GPL-2.0 DAC driver + overlay source, DKMS deb), `scripts/` (offset export, Wi-Fi inject, offline image checks).

## Wi-Fi after flashing (Mac)

The image has no user, password or Wi-Fi credentials. After writing the card, re-insert it (volume `bootfs`).

Two steps. Paste **Step 1**, enter the ZPOD login password when OpenSSL asks (twice), then paste **Step 2**.

Needs OpenSSL 3 (`openssl passwd -6`). macOS `/usr/bin/openssl` has no `-6`; use conda or Homebrew OpenSSL. Required unless you use a key-only setup (`zpod-ssh.pub` only).

**Step 1** — login user + password:

```bash
H=$(openssl passwd -6) && case "$H" in '$6$'*) printf 'paul:%s\n' "$H" > /Volumes/bootfs/userconf.txt ;; *) echo "use OpenSSL 3 (e.g. conda); macOS openssl has no -6"; exit 1 ;; esac; unset H
head -c 9 /Volumes/bootfs/userconf.txt; echo
```

**Step 2** — SSH key, Wi-Fi, eject (no password prompt):

```bash
cp ~/.ssh/id_ed25519.pub /Volumes/bootfs/zpod-ssh.pub
printf 'SSID=%s\nPSK=%s\n' '<SSID>' '<PW>' > /Volumes/bootfs/zpod-wifi.txt
ls /Volumes/bootfs/userconf.txt /Volumes/bootfs/zpod-ssh.pub /Volumes/bootfs/zpod-wifi.txt
diskutil eject /Volumes/bootfs
```

Replace `<SSID>` and `<PW>` with your network name and password before running Step 2. Wi-Fi is optional (USB networking works without it; the file is imported at first boot then deleted). SSH key is optional (public key only).

Then `ssh paul@zpod.local` (Wi-Fi) or `ssh paul@10.12.194.1` (USB cable on the data port). To add music, run `scripts/zpod-push-music.sh ~/Music/Artist`, or open `http://10.12.194.1:8080/` over USB.

On first boot the ZPOD imports `zpod-wifi.txt` into NetworkManager and deletes
it. Without it, use the USB cable: `ssh paul@10.12.194.1`. Details:
[docs/BOOT-WIFI.md](./docs/BOOT-WIFI.md).

## Do not

- Flash an SD card from build scripts (write cards yourself).
- Invent pinouts.
- Claim SLSA levels the pipeline has not earned.
- Use the `elbmyn` GitHub login for this project.

## Build scaffold

A local pi-gen scaffold also lives on the agent computer at `/workspace/zpod-image`. Useful pieces will be merged here as the pipeline lands. `./build.sh` without flags must remain a no-op.

## Hardware

The uGeek / AOIDE **ZPOD** is a pocket-size handheld audio player in a black rectangular case with a small colour LCD on the front. The 2017 vendor user guide ([docs/vendor/ZPOD-HiFi-Music-Player-User-Guide-2017.pdf](./docs/vendor/ZPOD-HiFi-Music-Player-User-Guide-2017.pdf), uGeek, third-party, kept for reference only) is the source for the physical layout below. It describes the original software, which this project does not use, and says nothing about the SoC, DAC, battery or dimensions, so those rows come from our own on-hardware notes and are marked as such.

**Controls and ports (source: the vendor guide's photos and labels, restated here)**

| Item | What is on the device |
| --- | --- |
| Front controls | A white four-way pad with a centre press, four face buttons marked triangle, circle, square and cross, a Home button, a Play/Pause button, and a volume up and a volume down button |
| Power | A slide switch for power on and off |
| Charging | One micro-USB socket, marked with a lightning symbol, for charging |
| Data | A second micro-USB socket. The guide drives it as a host port, through adapters, for a USB stick, a Wi-Fi module or a USB Ethernet adapter |
| Storage | A microSD slot; the system software boots from the card |
| Audio out | A headphone jack |
| Video out | A micro-HDMI socket |
| Display | A small colour LCD on the front, with the AOIDE and ZPOD marks |

**Not stated in the vendor guide.** Battery capacity, case dimensions, weight and a formal spec sheet. They are not recorded in this repository either.

**What our own tests have confirmed (not from the PDF)**

| Part | Detail | Where |
| --- | --- | --- |
| Computer | Raspberry Pi Zero / Zero W | [docs/stock-introspection.md](./docs/stock-introspection.md) |
| DAC | TI PCM5122 on I2S, with an external headphone amp that the driver un-mutes | [drivers/aoide-zpod-dac/README.md](./drivers/aoide-zpod-dac/README.md) |
| Display | 320x240 ILI9340 LCD on SPI | [docs/APPS.md](./docs/APPS.md) |
| Battery gauge | MAX17048 on I2C1, address 0x36 | [docs/BATTERY.md](./docs/BATTERY.md) |
| Button wiring | BCM GPIO map for the pad, face, Home, Play/Pause and volume buttons | [docs/BUTTONS.md](./docs/BUTTONS.md) |

## Licence

Copyright (C) 2026 Paul Lambert. Dual licensed, SPDX `GPL-3.0-or-later OR LicenseRef-Commercial`:

- **GPL-3.0-or-later**: [LICENSE](./LICENSE) (the full GPL text);
- **LicenseRef-Commercial**: commercial terms from the copyright holder, see [LICENSE-COMMERCIAL.md](./LICENSE-COMMERCIAL.md) (enquiries: support@m-of-n.com).

Exceptions, third-party material and the MIT history of earlier releases are in [NOTICE.md](./NOTICE.md). `drivers/aoide-zpod-dac/` is GPL-2.0-only and is not dual-licensed. Licence texts are in [LICENSES/](./LICENSES/); [REUSE.toml](./REUSE.toml) records files that cannot carry a header. Contributing: [CONTRIBUTING.md](./CONTRIBUTING.md) (interim terms).

## Credits

- **Howard Q. (Howard Qiao)**, <https://github.com/howardqiao>, created the original uGeek / AOIDE ZPOD software: the player, setup scripts, and the screen and input configuration. He also published the AOIDE DAC drivers, including `aoide-zpod-dac`. His repos, their licenses, and what we use from each are in [docs/vendor/HOWARDQIAO.md](./docs/vendor/HOWARDQIAO.md).
- `drivers/aoide-zpod-dac/` is a GPL-2.0 out-of-tree rebuild of the vendor `aoide-zpod-dac` ASoC module (`MODULE_LICENSE("GPL v2")`, `MODULE_AUTHOR("WWW.AOIDE.CC")`), first distributed as prebuilt modules in [howardqiao/aoide-dac-drivers](https://github.com/howardqiao/aoide-dac-drivers) and [howardqiao/zpod](https://github.com/howardqiao/zpod). The overlay source compiles byte-identical to the vendor `aoide-zpod-dac.dtbo`.
- Hardware: uGeek ZPOD (AOIDE ZPOD DAC, PCM5122). The 2017 vendor user guide is in [docs/vendor/](./docs/vendor/README.md).

## Build and test docs

- [docs/BUILD.md](./docs/BUILD.md) — repeatable construction recipe and pins
- [docs/TEST-M1.md](./docs/TEST-M1.md) — first-install validation checklist
- [docs/TEST-M3.md](./docs/TEST-M3.md) — M3 on-device checklist
- [docs/TEST-M2.md](./docs/TEST-M2.md) — M2 on-device checklist
- [docs/BUTTONS.md](./docs/BUTTONS.md) — button → key code map
- [docs/ui/BUTTONS-AND-MENUS.md](./docs/ui/BUTTONS-AND-MENUS.md) — M4 button state machine, menu tree, app conventions
- [docs/plan/ROADMAP.md](./docs/plan/ROADMAP.md) — milestones in small pieces, action items, latest hardware results
- [docs/TEST-M4.md](./docs/TEST-M4.md) — M4 apps + candidate image checklist
- [docs/USB-GADGET.md](./docs/USB-GADGET.md) — USB cable networking from a Mac
- [docs/BUILD-RECORD.md](./docs/BUILD-RECORD.md) — what was built, checksums
