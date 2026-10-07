# zpod

Firmware and image pipeline for the **uGeek ZPOD** (Raspberry Pi Zero / Zero W portable player).

Public home: this repository under [nymble](https://github.com/nymble).

## Status

Early planning. No published SD image yet. Requirements and staged plan:

- [REQUIREMENTS.md](./REQUIREMENTS.md)
- [PLAN.md](./PLAN.md)
- [docs/stock-introspection.md](./docs/stock-introspection.md)

## What we are building

A new armhf image family (pi-gen), not a byte-for-byte rebuild of the 2017 SourceForge images. First bring-up uses **micro HDMI**. TFT, buttons, and battery wiring stay unset until verified on hardware. Audio path targets the vendor DAC overlay `aoide-zpod-dac` (TI PCM5122).

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
