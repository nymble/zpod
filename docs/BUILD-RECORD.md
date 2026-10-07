# BUILD-RECORD: ZPOD M2 pi0w (display + audio + buttons) — `v0.3.0-m2`

**Status:** Built, exported, verified **offline** (46/46 checks), published. **Not flashed / not hardware-tested** — Paul flashes; see [TEST-M2.md](./TEST-M2.md).
**Date:** 2026-10-07 (stage2 14:10:41–14:19:43 PT; export finished ~14:26 PT)

| Field | Value |
| --- | --- |
| zpod repo commit (sources built) | f5cf5c0627e7f39f069744543bfdedb0bb9abc6a |
| pi-gen commit | 59b67461533c2eef3564bc48eb3ce5c16e5c62ee (master, trixie armhf) |
| Board | pi0w (armhf); kernels 6.18.50+rpt-rpi-v6 / -v7 (+ arm64 -v8 shipped by RPi OS, unused on Zero W) |
| Stages | stage0 (SKIP) stage1 (SKIP) stage2 rebuilt with `build/tweaks` + **`05-zpod-m2`** |
| ENABLE_SSH / ENABLE_CLOUD_INIT / WPA_COUNTRY | 1 / 0 / US |
| Home SSID | configured via gitignored `config/wifi.local.env` (not recorded here); NM profile now has `powersave=2` |
| Hostname / user | zpod / `pi`, no baked password; userconfig.service enabled (create user with `userconf.txt` — see TEST-M2 §0) |
| Export method | `scripts/export-image-offset.sh` (offset losetup + finalize + Wi-Fi inject), `IMG_DATE=2026-10-07`, name `zpod-pi0w-m2` |
| Disk signature | 7dfbc2ca → PARTUUID=7dfbc2ca-01 (boot), 7dfbc2ca-02 (root) |
| Logs (box) | `/workspace/zpod-build-logs/m2-stage2-*.log`, `m2-export-*.log`, `m2-verify-final.log` |

## Artifacts

| File | Bytes | SHA-256 |
| --- | --- | --- |
| 2026-10-07-zpod-pi0w-m2.img.xz | 399389572 (381 MiB) | 3ebeb1ff3a8bcba3346e3806da34996a57433acd8f06d90f33f10fbbc071f0cc |
| 2026-10-07-zpod-pi0w-m2.img | 2592079872 (2.4 GiB) | 0e45154d24390296846358044a49e4379e025783162f58b6a4156a4863d86244 |

## What M2 adds (vs M1.1)

- **Audio:** `aoide-zpod-dac-dkms 1.0.0-1` → DKMS-built `aoide-zpod-dac.ko.xz` for `6.18.50+rpt-rpi-v6` (vermagic `ARMv6 p2v8`) and `-rpi-v7`; overlay `aoide-zpod-dac.dtbo` sha256 `3ad223aa…c069` (= vendor). config.txt: `dtparam=i2c_arm=on`, `i2s=on`, `spi=on`, `audio=off` (base-DT, before first `dtoverlay=`), `[all]` `dtoverlay=aoide-zpod-dac`, `gpio=14=op,dl`, `gpio=4,5,6,12,13,16,17,22,23,24,26=ip,pu`. cmdline: no `console=serial0`. `/etc/asound.conf` default = `sndrpiaoidezpod`; first boot sets Digital 80 %, Analogue Playback Boost on.
- **Display:** `zpod-ui.service` (spidev0.0 32 MHz, DC 25, GPIO27 high, ILI9340-style init, MADCTL 0xE8, 320×240).
- **Buttons:** `zpod-buttons.service` → uinput `zpod-buttons` ([BUTTONS.md](./BUTTONS.md)).
- **USB:** `dtoverlay=dwc2,dr_mode=peripheral`, `g_ether` (fixed MACs), NM `USB Gadget (shared)` 10.12.194.1/28 + `(client)`, `rpi-usb-gadget-ics` enabled ([USB-GADGET.md](./USB-GADGET.md)).
- **Wi-Fi:** NM `wifi.powersave = 2`.
- **Debug:** persistent journald (64 MB), `i2c-tools`, `gpiod`, `iw`, `evtest`.

**Packages added (stage2 delta vs M1.1):** `aoide-zpod-dac-dkms`, `dkms`, `build-essential` + `gcc-14`/`g++-14`/`make`/`patch`/`dpkg-dev` (pulled in by DKMS; needed to rebuild on kernel upgrades), `python3-pil` (+ freetype/png/tiff/webp libs), `python3-evdev`, `fonts-dejavu-core`, `evtest`. Kernel headers rpi-v6/v7 were already present.

## Size vs M1.1

| Metric | M1.1 | M2 | Delta |
| --- | --- | --- | --- |
| stage2 rootfs (export `du`) | 1431277568 | 1529987072 | +94 MiB (DKMS toolchain ≈ most of it) |
| .img | 2474639360 | 2592079872 | +112 MiB |
| .img.xz | 385691184 | 399389572 | +13 MiB |

## Offline verification (2026-10-07 ~14:26 PT)

`sudo scripts/verify-m2-image.sh build/pi-gen/deploy/m2/2026-10-07-zpod-pi0w-m2.img` → **46 passed, 0 failed**:
cmdline (no serial console, PARTUUID), all config.txt lines and their order, dtbo hash, dwc2 overlay present, DKMS modules for rpi-v6 + rpi-v7, `dkms status`, `modinfo` alias `of:…aoide,aoide-zpod-dac`, services enabled (zpod-ui, zpod-buttons, zpod-audio-init, rpi-usb-gadget-ics), asound.conf, NM powersave (global + home profile), usb0 profiles (mode 600) + managed rule, g_ether/uinput module loading, persistent journal, tools, no desktop, no baked password, userconfig enabled, qemu-chroot Python imports (spidev, lgpio, evdev, PIL 11.1.0, smbus2, all zpod modules) + RGB565 conversion, `systemd-analyze verify` of the units. Also rendered every UI screen inside the image chroot (fake LCD) and simulated the button state machine (`tests/`). `sha256sum -c` and `xz -t` OK.

**Not verifiable without hardware:** LCD output on the real panel from the service, button events from real GPIOs (no `/dev/uinput` on the build host), DAC playback through the DKMS module, MAX17048 readout, USB gadget enumeration on macOS, Wi-Fi stability in the case.

## Published

| Item | URL |
| --- | --- |
| Source commit | https://github.com/nymble/zpod/commit/f5cf5c0627e7f39f069744543bfdedb0bb9abc6a |
| Release | https://github.com/nymble/zpod/releases/tag/v0.3.0-m2 |
| Issue #4 (audio) | https://github.com/nymble/zpod/issues/4#issuecomment-6047200037 |
| Issue #6 (display/buttons) | https://github.com/nymble/zpod/issues/6#issuecomment-6047200288 |
| Issue #12 (introspect) | https://github.com/nymble/zpod/issues/12#issuecomment-6047200533 |
| Issue #14 (rapid slice) | https://github.com/nymble/zpod/issues/14#issuecomment-6047200812 |
| Issue #7 (USB file path) | https://github.com/nymble/zpod/issues/7#issuecomment-6047201187 |
| Issue #13 (lighten) | https://github.com/nymble/zpod/issues/13#issuecomment-6047201436 |

## Previous: ZPOD M1.1 pi0w (Wi-Fi + lighten)

**Status:** BOOTABLE (verified offline). Exported + checksums OK. Published to GitHub Release. **Not flashed.**
**Date:** 2026-10-07 (stage2 08:23–08:30 PT; export finished ~08:36 PT)

| Field | Value |
| --- | --- |
| zpod repo commit | b106bf23afdbcd0107da32739a964c1cae9645b2 |
| pi-gen commit | 59b67461533c2eef3564bc48eb3ce5c16e5c62ee |
| Board | pi0w (armhf); also for Zero 2 W HDMI/Wi-Fi tests |
| Stages | stage0 (SKIP) stage1 (SKIP) stage2 rebuild with `build/tweaks` |
| ENABLE_SSH | 1 |
| ENABLE_CLOUD_INIT | 0 |
| WPA_COUNTRY | US |
| Home SSID | configured via gitignored `config/wifi.local.env` → NM `preconfigured.nmconnection` + boot `wpa_supplicant.conf` (SSID/password not recorded here) |
| Hostname | zpod |
| User | pi, no baked password; userconfig.service **enabled** |
| Export method | offset-losetup + finalize (PARTUUID, rename-user, resolv, finalise subset) + Wi-Fi inject |
| Disk signature IMGID | d32a4eae → PARTUUID=d32a4eae-01 (boot), d32a4eae-02 (root) |
| Build duration | stage2 ~6.5 min (08:23:47–08:30:17 PT); export ~5 min (rsync+xz) |

## Size vs M1 baseline

| Metric | M1 (2026-10-06) | M1.1 (2026-10-07) | Delta |
| --- | --- | --- | --- |
| stage2 rootfs | 2139627520 (~2.0 GiB) | 1431277568 (~1.4 GiB) | **−670 MiB (−31%)** |
| .img | 3330277376 (3.1 GiB) | 2474639360 (2.4 GiB) | **−816 MiB** |
| .img.xz | 510178196 (487 MiB) | 385691184 (368 MiB) | **−119 MiB (−24%)** |

Exact xz bytes: run `stat` / SHA256SUMS on deploy artifact.

## Package changes (vs stock stage2 / M1)

**Added:** `i2c-tools`

**Removed (lighten):** `build-essential`, `manpages-dev`, `gdb`, `pkg-config`, `man-db`, `apt-listchanges`, `lua5.1`, `luajit`, `rpicam-apps-lite`, `mkvtoolnix`, `cifs-utils`, `rpi-connect-lite`, `pciutils`, `p7zip-full`, `kms++-utils`, `rpi-update`, `rpi-eeprom`, `cloud-init` (+ mods), non-brcm firmwares (`firmware-atheros`, `firmware-libertas`, `firmware-realtek`, `firmware-mediatek`, `firmware-marvell-prestera`)

**Kept:** `firmware-brcm80211`, `wpasupplicant`, `network-manager`, `raspberrypi-net-mods`, `openssh-server`/`ssh`, `bluez`, GPIO/I2C Python stack

## Offline verification (2026-10-07 ~08:36 PT)

- cmdline.txt: `root=PARTUUID=d32a4eae-02`, `cfg80211.ieee80211_regdom=US`
- fstab: PARTUUID boot+root (no BOOTDEV/ROOTDEV)
- userconfig.service enabled; ssh.service enabled; `i2cdetect` present
- boot FAT: `wpa_supplicant.conf` present (home SSID); root: NM preconfigured.nmconnection mode 600
- No invented TFT/DAC overlays

## Artifacts

| File | SHA-256 |
| --- | --- |
| 2026-10-07-zpod-pi0w.img.xz | fb58297d50995b039ab2a7216f1e5a30087fef41b72163015fb945bab7faa728 |
| 2026-10-07-zpod-pi0w.img | 1ac927f1508f6b204a8cbbb8ed4fd0bf2f8838a0a4744913e077f343caa9bac4 |

## Published

| Item | URL |
| --- | --- |
| Recipe commit | https://github.com/nymble/zpod/commit/b106bf23afdbcd0107da32739a964c1cae9645b2 |
| Docs pin commit | https://github.com/nymble/zpod/commit/229fbe66e058784d8d3cbe3c458743ed24b6c30f |
| Release | https://github.com/nymble/zpod/releases/tag/v0.2.0-m1.1-wifi |
| Issue #13 | https://github.com/nymble/zpod/issues/13 |
| Issue #3 | https://github.com/nymble/zpod/issues/3 |

| Commit (docs pin) | https://github.com/nymble/zpod/commit/229fbe66e058784d8d3cbe3c458743ed24b6c30f |
| Issue #13 comment | (see issue timeline) |
| Issue #3 comment | (see issue timeline) |
