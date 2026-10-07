# zpod

Firmware and image pipeline for the **uGeek ZPOD** (Raspberry Pi Zero / Zero W portable player).

Public home: this repository under [nymble](https://github.com/nymble).

## Status

Published images ([Releases](https://github.com/nymble/zpod/releases)):

| Release | What |
| --- | --- |
| `v0.3.0-m2` | **M2**: LCD status/menu UI (`zpod-ui`), buttons → keyboard (`zpod-buttons`), Aoide PCM5122 DAC via DKMS, USB-cable Ethernet to a Mac, Wi-Fi power save off — test with [docs/TEST-M2.md](./docs/TEST-M2.md) |
| `v0.2.0-m1.1-wifi` | M1.1: lite trixie armhf + Wi-Fi stack |

Requirements and staged plan:

- [REQUIREMENTS.md](./REQUIREMENTS.md)
- [PLAN.md](./PLAN.md)
- [docs/stock-introspection.md](./docs/stock-introspection.md)

## What we are building

A new armhf image family (pi-gen), not a byte-for-byte rebuild of the 2017 SourceForge images. Display, buttons, DAC and fuel gauge use only the pin maps confirmed on hardware on 2026-10-07 ([stock-introspection](./docs/stock-introspection.md)).

Repo layout: `build/` (pi-gen tweaks + M2 substep), `overlay/` (files installed into the image: `zpod-ui`, `zpod-buttons`, configs), `drivers/aoide-zpod-dac/` (GPL-2.0 DAC driver + overlay source, DKMS deb), `scripts/` (offset export, Wi-Fi inject, offline image checks).

## Do not

- Flash an SD card from build scripts (write cards yourself).
- Invent pinouts.
- Claim SLSA levels the pipeline has not earned.
- Use the `elbmyn` GitHub login for this project.

## Build scaffold

A local pi-gen scaffold also lives on the agent computer at `/workspace/zpod-image`. Useful pieces will be merged here as the pipeline lands. `./build.sh` without flags must remain a no-op.

## Build and test docs

- [docs/BUILD.md](./docs/BUILD.md) — repeatable construction recipe and pins
- [docs/TEST-M1.md](./docs/TEST-M1.md) — first-install validation checklist
- [docs/TEST-M2.md](./docs/TEST-M2.md) — M2 on-device checklist
- [docs/BUTTONS.md](./docs/BUTTONS.md) — button → key code map
- [docs/USB-GADGET.md](./docs/USB-GADGET.md) — USB cable networking from a Mac
- [docs/BUILD-RECORD.md](./docs/BUILD-RECORD.md) — what was built, checksums
