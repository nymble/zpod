# BUILD-RECORD: ZPOD M1.1 pi0w (Wi-Fi + lighten)

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
| Release | https://github.com/nymble/zpod/releases/tag/v0.2.0-m1.1-wifi |
