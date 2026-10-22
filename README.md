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

The image has no Wi-Fi credentials. After writing the card, re-insert it and run:

```bash
echo "paul:$(openssl passwd -6)" > /Volumes/bootfs/userconf.txt               # create your user (prompts)
printf 'SSID=%s\nPSK=%s\n' 'YourSSID' 'YourPassword' > /Volumes/bootfs/zpod-wifi.txt   # optional Wi-Fi
```

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
