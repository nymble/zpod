# Repeatable image build

This document is the construction recipe for ZPOD SD images produced from this repo.
It is the human-readable twin of `./build.sh`. Follow it exactly so a second machine can reproduce the same class of artifact.

## Scope of M1.1 (Wi-Fi + lighten)

- Same base as M1: **pi0w** armhf, stages `stage0 stage1 stage2`, hostname `zpod`,
  user `pi` with no baked password, `ENABLE_SSH=1`, no desktop, no TFT/DAC overlays.
- **Wi-Fi ready:** `WPA_COUNTRY=US`, `firmware-brcm80211`, NetworkManager +
  wpa_supplicant. SSID/password **not** baked in — see [BOOT-WIFI.md](./BOOT-WIFI.md).
- **Adds:** `i2c-tools`.
- **Lightens:** drops unused Wi-Fi firmwares (non-brcm), build/dev packages,
  camera/video extras, cloud-init packages, and other console-unused defaults.
  See `build/tweaks/`.
- `./build.sh --confirm --board pi0w` applies `build/tweaks/apply.sh` onto the
  pi-gen clone before writing `build/pi-gen-config`.

## Scope of the first image (M1)


- Board profile: **pi0w** (also supports flashing a Pi Zero for HDMI-only tests; onboard radio is unused until configured).
- Architecture: **armhf** (BCM2835). Not arm64.
- Builder: [RPi-Distro/pi-gen](https://github.com/RPi-Distro/pi-gen) branch `master`.
- Stages: `stage0 stage1 stage2` (Raspberry Pi OS lite-class).
- Hostname: `zpod`.
- User: `pi` with **no baked password** (first-boot wizard on HDMI sets it).
- SSH: enabled (`ENABLE_SSH=1`).
- Compression: `xz`.
- Out of scope for M1: TFT, buttons, DAC overlays, SLSA provenance claim. (M1.1 adds Wi-Fi country + stack; still no SSID.)

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

## Host note: armhf on x86_64

pi-gen requires working `binfmt_misc` so `sudo arch-test -c <tmpdir> armhf` prints `armhf: ok`.
On hosts where the `binfmt_misc` kernel module is missing, mount the filesystem and register QEMU with the **F** flag:

```
sudo mount -t binfmt_misc binfmt_misc /proc/sys/fs/binfmt_misc
printf '%s\n' ':qemu-arm:M::\x7fELF\x01\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02\x00\x28\x00:\xff\xff\xff\xff\xff\xff\xff\x00\xff\xff\xff\xff\xff\xff\xff\xff\xfe\xff\xff\xff:/usr/bin/qemu-arm-static:F' | sudo tee /proc/sys/fs/binfmt_misc/register
```

`./build.sh --build` must run with cwd inside the pi-gen checkout (the scaffold does this). Relative `STAGE_LIST` values fail if cwd is the outer repo.

## Export fallback: offset losetup (no loop partition nodes)

Stock pi-gen export expects `/dev/loopNpM` partition devices after `losetup -P`.
Some restricted hosts (containers, kernels without loop partition scan) never create
those nodes. Installing or reloading **udev** rules alone is **not** enough when the
kernel does not perform loop partition scanning — there is nothing for udev to name.

In that case, after stages 0–2 succeed, export with:

```
sudo ./scripts/export-image-offset.sh \
  build/pi-gen/work/zpod-pi0w/stage2/rootfs \
  build/pi-gen/deploy \
  zpod-pi0w
```

What the script does:

1. Sizes boot (512 MiB FAT) + root (rootfs + 20% + 200 MiB) aligned to 8 MiB.
2. Creates an `.img`, partitions with `parted`, and attaches two loop devices via
   `losetup --offset` / `--sizelimit` (no `-P` / no `/dev/loopNpM`).
3. Formats FAT + empty ext4; `rsync`s the stage2 rootfs into the mounted ext4 root
   (excluding apt archives and `boot/firmware` contents).
4. **Finalize** (pi-gen `export-image` 01/03/04/05 subset — not a raw stage2 dump):
   - `04-set-partuuid`: replace `BOOTDEV`/`ROOTDEV` in fstab with `PARTUUID=<diskid>-01/-02`
   - `01-user-rename`: chroot `rename-user -f -s` so `userconfig.service` is enabled
     (HDMI first-boot password wizard; `pi` has no baked password)
   - `03-network`: install `resolv.conf`
   - `05-finalise` subset: restore `ld.so.preload`, set `machine-id` to `uninitialized`,
     clear logs, drop `passwd-` backups, fix `mtab`. Skips `update-initramfs`,
     apt dist-upgrade, fstrim, and info/sbom generation.
5. Populates the FAT boot partition with **mtools** using `file@@offset` addressing
   (`IMG_FILE@@BOOT_PART_START`), not `/dev/loopN`. On this class of host, mtools
   against the loop node fails geometry ioctls and can silently copy nothing;
   there is also often **no kernel `vfat` module**, so `mount -t vfat` is unavailable.
6. Overwrites `cmdline.txt` on the FAT with the PARTUUID-patched root= line
   (must not leave `ROOTDEV`).
7. Compresses with `xz -T0 -9 -k`, writes `SHA256SUMS`, prints `EXPORT_OK <path>`.

Notes:

- `du` is run **without** `-x`. On overlayfs, lower/upper layers can disagree on
  `st_dev`, so one-file-system mode under-counts (often to 0) and the image is
  sized too small.
- Requires `xxd` and `qemu-arm-static` (for the rename-user chroot); cleanup
  unmounts bind mounts (`/dev`, `/proc`, `/sys`) before detaching the root loop.
- Still does **not** flash a card. Flash yourself per [TEST-M1.md](./TEST-M1.md)
  (`xz -d` then Raspberry Pi Imager / `dd`; first-boot HDMI wizard sets the `pi` password).
