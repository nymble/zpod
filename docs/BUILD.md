# Repeatable image build

This document is the construction recipe for ZPOD SD images produced from this repo.
It is the human-readable twin of `./build.sh`. Follow it exactly so a second machine can reproduce the same class of artifact.

## Scope of the first image (M1)

- Board profile: **pi0w** (also supports flashing a Pi Zero for HDMI-only tests; onboard radio is unused until configured).
- Architecture: **armhf** (BCM2835). Not arm64.
- Builder: [RPi-Distro/pi-gen](https://github.com/RPi-Distro/pi-gen) branch `master`.
- Stages: `stage0 stage1 stage2` (Raspberry Pi OS lite-class).
- Hostname: `zpod`.
- User: `pi` with **no baked password** (first-boot wizard on HDMI sets it).
- SSH: enabled (`ENABLE_SSH=1`).
- Compression: `xz`.
- Out of scope for M1: TFT, buttons, DAC overlays, Wi-Fi country code, SLSA provenance claim.

Pinned pi-gen commit used for the 2026-10-06 agent build:

```
repo:   https://github.com/RPi-Distro/pi-gen.git
branch: master
commit: 59b67461533c2eef3564bc48eb3ce5c16e5c62ee
```

Re-check and update this pin when you re-clone.

## Host requirements

- Debian-like x86_64 host with `sudo`, tens of GB free disk (aim for ≥40 GB free).
- Packages (from pi-gen README / `build.sh` warning list):

```
sudo apt-get install -y \
  coreutils quilt parted qemu-user-static qemu-user-binfmt debootstrap zerofree zip \
  dosfstools e2fsprogs libarchive-tools libcap2-bin rsync xz-utils file git curl bc gpg \
  pigz xxd arch-test bmap-tools kmod fdisk gawk
```

This script never flashes an SD card. Write the card yourself with a tool you trust onto a **disposable** card.

## Construction steps

1. Clone this repo and enter it.
2. Confirm scaffold only (no clone if you only want help text):
   ```
   ./build.sh
   ```
3. Prepare config and clone pi-gen (does not build):
   ```
   ./build.sh --confirm --board pi0w
   ```
4. Record the pi-gen commit:
   ```
   git -C build/pi-gen rev-parse HEAD
   ```
   Update the pin in this file if it differs.
5. For M1 first-install test, ensure the generated `build/pi-gen-config` contains:
   - `IMG_NAME='zpod-pi0w'` (or document the name you chose)
   - `ENABLE_SSH=1`
   - `STAGE_LIST='stage0 stage1 stage2'`
   - empty / absent `FIRST_USER_PASS`
   - empty `WPA_COUNTRY` unless Paul set a country code on purpose
6. Start the build (hours; uses sudo and loop devices):
   ```
   ./build.sh --confirm --board pi0w --build
   ```
7. On success, pi-gen writes deploy artifacts under `build/pi-gen/deploy/` (typical names like `*-zpod-pi0w-*.img.xz`).
8. Compute and keep checksums next to the file:
   ```
   cd build/pi-gen/deploy
   sha256sum *.img.xz | tee SHA256SUMS
   xz -l *.img.xz
   ```
9. Publish: attach `*.img.xz` + `SHA256SUMS` to a GitHub Release; link from Pages when M0 exists. Do not commit multi-hundred-MB images into git.

## What makes a rebuild “the same”

Same inputs means:

| Input | Where recorded |
| --- | --- |
| This repo commit | `git rev-parse HEAD` |
| pi-gen commit | pin above + `git -C build/pi-gen rev-parse HEAD` |
| Board id | `pi0` or `pi0w` on the `build.sh` command line |
| `build/pi-gen-config` contents | regenerate via `--confirm`, then diff before `--build` |
| Host package set | Debian release + apt package list (optional dump for M6) |

Bit-identical images are **not** promised at M1 (timestamps, package mirrors). M6 raises the bar to SBOM + provenance. Until then, “repeatable” means same recipe, same pins, documented checksum of the artifact you ship.

## Validation of contents

See [TEST-M1.md](./TEST-M1.md) for the first-install hardware test plan.

Offline checks before flashing:

1. `sha256sum -c SHA256SUMS`
2. `xz -t` on the `.img.xz`
3. Optional: decompress and `fdisk -l` / mount boot partition read-only; confirm `cmdline.txt`, hostname file, and that no TFT/DAC overlays were added by mistake.

## Safety

- Never invent TFT/button/DAC pinouts in the image.
- Never use the `elbmyn` GitHub account for releases.
- Never flash from `build.sh`.
