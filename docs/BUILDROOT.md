# Buildroot fast-boot image (M6 test)

**Status: test image, built and booted in QEMU, not yet booted on hardware (2026-10-30).**
Tracking issue: [#22](https://github.com/nymble/zpod/issues/22). Branch: `buildroot`, draft PR [#23](https://github.com/nymble/zpod/pull/23).
Prereleases: [`v0.8.0-br-m6-fastboot-test1`](https://github.com/nymble/zpod/releases/tag/v0.8.0-br-m6-fastboot-test1) (M6 test, this document), [`v0.7.0-br-poc1`](https://github.com/nymble/zpod/releases/tag/v0.7.0-br-poc1) (first PoC).

This is the second OS track for the ZPOD: a Buildroot image with BusyBox
init, a read-only squashfs root and a writable `/data` partition, running the
same Rust launcher and apps as the Pi OS (pi-gen) image. The goal is **power-on
to launcher in about 5 s** (estimate) instead of about 1.5 min (pi-gen M3/M4,
measured on hardware 2026-10-22: menu after about 1 min 32 s). M6 brings it
close to feature parity with the Pi OS image so it can replace it later.

The Pi OS image stays the supported image. Nothing here changes it.

## Build

Needs a Linux build host or container: a case-sensitive filesystem and about
25 GB free. On a Mac, use a container in a colima VM. Do not use the macOS
filesystem for the Buildroot output.

```sh
# Mac, one time (as built on 2026-10-29: Stellas-Mac, M4, colima 0.10.3, vz)
brew install colima docker
colima start --profile zpod-br --vm-type vz --cpu 5 --memory 8 --disk 80 --network-address
docker --context colima-zpod-br volume create zpod-br-work
docker --context colima-zpod-br run -d --name zpod-br -v zpod-br-work:/br -v ~/src/zpod:/src/zpod debian:trixie sleep infinity
docker --context colima-zpod-br exec zpod-br sh -c 'apt-get update && apt-get install -y build-essential git wget curl cpio unzip rsync bc file python3 libncurses-dev libssl-dev perl xz-utils bzip2 patch which device-tree-compiler mtools dosfstools e2fsprogs squashfs-tools fdisk lz4 binutils-arm-linux-gnueabihf'
# Rust for the apps (rustup + musl target, zig 0.14.1, cargo-zigbuild)
docker --context colima-zpod-br exec zpod-br sh -c 'curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --target arm-unknown-linux-musleabihf && curl -sSL https://ziglang.org/download/0.14.1/zig-aarch64-linux-0.14.1.tar.xz | tar xJ -C /opt && ln -sf /opt/zig-aarch64-linux-0.14.1/zig /usr/local/bin/zig && . ~/.cargo/env && cargo install --locked cargo-zigbuild'

# every build
docker --context colima-zpod-br exec zpod-br sh -c 'cd /src/zpod && ZPOD_BR_WORK=/br buildroot/build.sh'
# -> /br/output/images/sdcard.img (inside the zpod-br-work volume)
```

`buildroot/build.sh` clones Buildroot **2026.02.3** (LTS) into
`$ZPOD_BR_WORK/buildroot` if it is missing. It then cross-builds the Rust apps
(`apps/tools/build-arm.sh`, in a private copy so a macOS `apps/target` is
never mixed in) into `apps/dist`, and runs `make zpod_pi0w_defconfig && make`.
Other targets pass through to make: `buildroot/build.sh linux-menuconfig`,
`buildroot/build.sh mpd-rebuild all`.

Tests (in the container): `sh buildroot/tests/test_firstuser_br.sh`,
`sh buildroot/tests/test_shims_br.sh`, and the QEMU smoke test
`sh buildroot/tests/qemu-boot.sh setup && sh buildroot/tests/qemu-boot.sh boot`.

A first build takes about 60–70 min on 5 cores of an M4 under colima. That
time is mostly the internal GCC 15 toolchain and the kernel. Incremental image
rebuilds take 1–3 min.

Networking note: with two colima VMs on one Mac (`zpod` for pi-gen, `zpod-br`
for Buildroot), the shared user-mode network (`user-v2`) lost outbound TCP in
both VMs. `--network-address` (vz NAT, `network.address: true` and
`preferredRoute: true` in `~/.colima/zpod-br/colima.yaml`) fixed it for
`zpod-br` without sudo.

## What is in the tree (`buildroot/`, a `BR2_EXTERNAL` named `ZPOD`)

| Path | What |
|---|---|
| `configs/zpod_pi0w_defconfig` | ARM1176JZF-S, internal glibc toolchain (GCC 15, C++, headers from the 6.12 kernel; Bootlin toolchains are x86_64-host only), RPi kernel 6.12.61 `bcmrpi` + fragment, rpi-firmware, brcmfmac Wi-Fi + BT firmware, BusyBox init + mdev, squashfs LZ4 root. Packages: ALSA, MPD (alsa, flac, mpg123, faad2, vorbis, opus, id3tag, sqlite) + mpc, python3 + evdev + smbus2, wpa_supplicant + wpa_cli, iw, dropbear, gesftpserver (macOS `scp` uses SFTP), rsync, BlueZ (bluetoothd, bluetoothctl, btmgmt) + D-Bus, sudo, util-linux (sfdisk, partx), i2c-tools, e2fsprogs, exfatprogs |
| `board/zpod/linux.fragment` | squashfs/ext4 built in (no initramfs), spidev + GPIO cdev, uinput, `RASPBERRYPI_GPIOMEM=y`, bcm2835 I2S + PCM512x, I2C chardev, **DS3231 built in with `RTC_HCTOSYS`**, dwc2 + configfs NCM/ECM/RNDIS/ACM/mass storage, brcmfmac, Bluetooth `hci_uart` serdev + BCM (modules), exFAT. **M6 trim:** no ftrace/kprobes/kallsyms/latencytop/debugfs/audit/BPF/cgroups/LSM, no NFS/F2FS/other unused file systems, no netfilter/qdiscs/IPsec, no DRM/media, USB host/SCSI/HID as modules (the data port is gadget-only). zImage **4.6 MB (was 9.4 MB)** |
| `board/zpod/config.txt` | the pi-gen hardware lines (I2C/I2S/SPI, `gpio=14=op,dl`, DC/backlight pins, button pull-ups, `dtoverlay=aoide-zpod-dac`, `i2c-rtc,ds3231`, `dwc2,dr_mode=peripheral`), plus `hdmi_ignore_hotplug=1`, `disable_splash=1`, `boot_delay=0`, `auto_initramfs=0`, `gpu_mem=16`. No `miniuart-bt`/`disable-bt` (see Bluetooth) |
| `board/zpod/cmdline.txt` | `root=/dev/mmcblk0p2 rootfstype=squashfs ro quiet loglevel=3 console=tty1` (no serial console: GPIO14 must stay low) |
| `package/zpod-dac` | `aoide-zpod-dac.ko` built from `drivers/aoide-zpod-dac/` (same source as the DKMS package) |
| `package/zpod-overlays` | `aoide-zpod-dac.dtbo` compiled from `drivers/aoide-zpod-dac/aoide-zpod-dac-overlay.dts` |
| `package/zpod-apps` | installs `apps/dist` (static musl ARMv6 launcher, bootsplash, drop, zpod-app + music/clock/tetris `.zpa`) |
| `package/zpod-scripts` | `overlay/usr/lib/zpod/{zpod_buttons.py, zpod_battery_guard.py, zpod_hw.py, zpod-usb-gadget.sh, zpod-audio-init.sh}`, `asound.conf`, `buttons.conf`, `ui.conf` from the pi-gen overlay (single source) |
| `board/zpod/rootfs-overlay/` | inittab, rcS with boot marks, init scripts, `zpod-data-early` / `zpod-data-grow`, `zpod-firstuser-br`, `zpod-wifi-import-br` + `zpod-wpa-psk`, mpd.conf, udhcpd, shims: `systemctl` (poweroff/reboot, the rsync share), `nmcli` (Wi-Fi scan/list on wpa_cli) |
| `board/zpod/post-build.sh` | read-only root plumbing: `/opt/zpod/apps → /data/zpod/apps`, `/var/lib/{mpd,zpod,alsa,seedrng,dbus,bluetooth} → /data/var/*`, `/root/.ssh → /data/etc/ssh`, `/etc/dropbear → /data/etc/dropbear`, sudoers, btmgmt, rsyncd.conf, mdev rules, `/etc/zpod-release` |
| `board/zpod/post-fakeroot.sh` | accounts on `/data`: `/etc/{passwd,group,shadow}` → `/data/etc/*` (base copies in `/usr/share/zpod/etc-base`), root password locked |
| `board/zpod/post-image.sh`, `genimage.cfg.in` | pre-installs the bundled apps into the data partition and assembles `sdcard.img` |
| `tests/` | `test_firstuser_br.sh` (26 cases), `test_shims_br.sh` (nmcli shim, data grow), `qemu-boot.sh` (QEMU smoke test) |

### Partition layout (MBR)

| # | Label | FS | Size | Mounted | Contents |
|---|---|---|---|---|---|
| p1 | ZPODBOOT | FAT32 | 64 MB | `/boot/firmware` (rw, 0700) | firmware, `zImage`, DTB, overlays, `config.txt`, `cmdline.txt`; first-boot files `userconf.txt`, `zpod-ssh.pub`, `zpod-user.txt`, `zpod-wifi.txt`, `zpod-usb.txt` |
| p2 | rootA | squashfs LZ4 | 256 MB partition (≈49 MB image) | `/` read-only | the OS |
| p3 | rootB | squashfs | 256 MB | – | **A/B stub**: same image as A, never booted (RAUC not done yet) |
| p4 | zpoddata | ext4 | 256 MB in the image, **grown to the end of the card at first boot** (max 2 TiB) | `/data` | music, playlists, app data, apps, accounts, SSH keys, dropbear host keys, Wi-Fi config, MPD/ALSA/BT state |

`sdcard.img` is 872 MB (832 MiB). Writable state lives only on `/data` and
tmpfs (`/run`, `/tmp`, `/var/log`).

**Same `/data` as Pi OS M5+:** ext4 labelled `zpoddata`, 4 KiB blocks, no
reserved blocks, 1 inode per 64 KiB, `-E resize=` for 2 TiB;
`/data/media/{music,playlists}` owned by uid 1000 (the login user),
`/data/zpod/appdata` (`ZPOD_DATA_DIR`) owned by `zpod`, and `/data/media`
**bind-mounted on `/srv/zpod/media`**. The difference is the partition number
(Pi OS: p3; here p4 because p2/p3 are the A/B root slots). The extra
Buildroot state (`/data/etc`, `/data/var`, `/data/home`, `/data/zpod/apps`) sits
next to it.

First boot: `zpod-data-early` (sysinit) mounts `/data` (it runs `mkfs.ext4`
only if p4 has no file system at all) and falls back to tmpfs if it cannot.
`S00zpod-data` then runs `zpod-data-grow` in the background: if more than
8 MiB is free after p4 it rewrites p4's size in the MBR (`sfdisk -N 4`),
tells the kernel (`partx -u`, works while mounted), and grows the file system
online (`resize2fs`). It is a no-op on later boots. Log: `/data/log/data-grow.log`.

### Boot sequence (BusyBox init)

1. `sysinit`: mount proc, sys and `/run`. Start **`zpod-bootsplash`** (Rust,
   spidev). `zpod-data-early` (`mount -a`, `/data`, accounts, `/srv/zpod/media`
   bind). Start the **launcher** in the background (`zpod-launcher-early`).
2. `rcS` runs `S00zpod-data` (layout, owners, device permissions, first login
   from `userconf.txt`/`zpod-ssh.pub`, `/data` grow in the background), seedrng,
   syslogd/klogd, `S10mdev` (module coldplug **in the background**),
   `S30zpod-hw` (buttons daemon, audio defaults, battery guard; background),
   `S35usbgadget` (CDC-NCM + udhcpd; background), `S40wifi` (`zpod-wifi.txt`
   import, wpa_supplicant, DHCP, NTP; background), `S45bluetooth` (hci0,
   D-Bus, bluetoothd; background), `S60zpod-drop`, `S61zpod-rsyncd` (only if
   enabled). Dropbear and MPD start in the background.
3. `respawn`: `zpod-launcher-run` waits for the early launcher and restarts it
   whenever it exits. `tty1` getty is for HDMI debugging (root has no
   password; log in as the `userconf.txt` user).
4. `shutdown`: `zpod-launcher-stop` sends SIGTERM (end screen), then `rcK`.

The launcher retries the input device every 2 s. So it can draw before the
python buttons daemon is up; the buttons work about 1–3 s later (estimate).

## First boot (no credentials in the image)

Same files as the Pi OS image ([FIRST-LOGIN.md](./FIRST-LOGIN.md),
[BOOT-WIFI.md](./BOOT-WIFI.md)); the volume is called **`ZPODBOOT`** instead
of `bootfs`. Write the image to a **spare** card (Raspberry Pi Imager "Use
custom", or `dd`; this repo never flashes cards), re-insert it, then on the Mac:

```zsh
# 1. login user + password (OpenSSL 3: macOS /usr/bin/openssl has no -6)
H=$(openssl passwd -6) && case "$H" in '$6$'*) printf 'paul:%s\n' "$H" > /Volumes/ZPODBOOT/userconf.txt ;; *) echo "use OpenSSL 3"; false ;; esac; unset H
# 2. SSH key (public key only)
cp ~/.ssh/id_ed25519.pub /Volumes/ZPODBOOT/zpod-ssh.pub
# 3. Wi-Fi (optional; replace the placeholders, keep the single quotes)
printf 'SSID=%s\nPSK=%s\n' '<SSID>' '<PW>' > /Volumes/ZPODBOOT/zpod-wifi.txt
# 4. check and eject
ls /Volumes/ZPODBOOT/userconf.txt /Volumes/ZPODBOOT/zpod-ssh.pub /Volumes/ZPODBOOT/zpod-wifi.txt
diskutil eject /Volumes/ZPODBOOT
```

What happens at first boot (`zpod-firstuser-br`, `zpod-wifi-import-br`):

* `userconf.txt` (`name:<sha512 hash>`) creates the login user (uid 1000,
  home `/data/home/<name>`, `/bin/sh`, groups audio/input/video/zpod/sudo)
  with that hash, and deletes the file. Dropbear then runs with `-g`:
  password logins for that user, **root stays key-only**. sudo asks for the
  password. An invalid file becomes `failed_userconf.txt`. A `userconf.txt`
  added on a later boot renames/sets the password of the existing uid 1000
  user (as on Pi OS).
* `zpod-ssh.pub`: public keys are added for the login user **and for root**
  (key-only fallback), then the file is deleted. Private keys are refused.
* `zpod-user.txt` + `zpod-ssh.pub` without `userconf.txt`: key-only login
  user with passwordless sudo (as on Pi OS). With only `zpod-ssh.pub`: root,
  key-only (dropbear `-s`, no password logins at all).
* `zpod-wifi.txt`: imported in the background by `S40wifi` into
  `/data/etc/wpa_supplicant.conf` (0600, SSID hex-encoded, only the derived
  WPA PSK; `zpod-wpa-psk` computes it from stdin), then shredded and deleted.

Log in over USB (CDC-NCM, Mac gets 10.12.194.2–14 by DHCP) or Wi-Fi:

```zsh
ssh paul@10.12.194.1      # or: ssh root@10.12.194.1 (key only)
zpod-boottime             # this boot's timing marks
```

Logs: `/data/log/firstuser.log`, `/data/log/data-grow.log`, `/run/zpod/*.log`.

## Wi-Fi without NetworkManager

`S40wifi` always starts wpa_supplicant on wlan0 (also with no network
configured), with its control socket at `/run/wpa_supplicant` (group `zpod`).
The launcher runs as root, so Tools > Wi-Fi scanner's `iw dev wlan0 scan`
works directly. Its fallback calls `nmcli device wifi rescan`; `/usr/bin/nmcli`
is a small shim that maps that (and `nmcli -t -f ACTIVE,SSID,SIGNAL device wifi
list`) to `wpa_cli scan` / `scan_results`. Wi-Fi info uses `iw dev wlan0 link`.
No app code changes were needed.

## Bluetooth and the UART (GPIO14)

The Zero W's BCM43430 Bluetooth is wired to the **PL011 UART on GPIO30–33
(internal; GPIO43 is its 32 kHz clock), which the device tree's `uart0_pins` claim**. The kernel's `hci_uart` serdev driver attaches it from the
device tree (`krnbt`, default on in this firmware), so no `hciattach`/`btuart`
is needed; `S45bluetooth` waits for hci0 and starts D-Bus and bluetoothd.
The Tools scanner uses `btmgmt` (installed from the BlueZ build) and falls back
to `bluetoothctl`.

GPIO14/15 carry the **mini UART**, which `enable_uart=0` leaves disabled, so
`gpio=14=op,dl` keeps GPIO14 LOW. **Do not add `dtoverlay=miniuart-bt` or
`dtoverlay=disable-bt`:** both move UART0 (PL011) onto GPIO14/15 (README:
"restore UART0/ttyAMA0 over GPIOs 14 & 15"), which would drive GPIO14 high as
TXD idle and mute the headphone amp. With neither overlay there is no conflict.
(Pi OS M5 uses the same default arrangement.)

## rsync

rsync over SSH works as on Pi OS (`scripts/zpod-push-music.sh`, as the login
user or root). The optional **rsync share** (no password, USB link only,
`10.12.194.1:873`, OFF by default) uses the same `/etc/zpod/rsyncd.conf`;
`S61zpod-rsyncd` runs `rsync --daemon` when `/data/etc/zpod/rsyncd.enabled`
exists. The launcher's File transfer screen toggles it through the
`systemctl` shim (`systemctl enable|disable --now zpod-rsyncd.socket`).

## Boot time

| Stage | Number | Kind |
|---|---|---|
| GPU firmware (bootcode.bin → start.elf → kernel load), with `hdmi_ignore_hotplug` | ~2.3–2.5 s | estimate (ahepp.dev, Zero W) |
| zImage load: **4.6 MB** (PoC: 9.4 MB) at ~6 MiB/s | ~0.8 s (PoC ~1.5 s) | estimate |
| kernel start → `init` | **1.0–1.2 s** in QEMU `raspi0` (PoC 2.6 s) | **measured in emulation** (TCG); hardware ~1–1.5 s, estimate |
| kernel start → launcher exec | **1.7–2.5 s** in QEMU over 4 boots (1.69, 2.10, 1.88, 2.52; PoC 2.90) | **measured in emulation** |
| kernel start → splash first frame | 1.97 s in QEMU (no real panel) | measured in emulation |
| launcher exec → first frame | not measurable in QEMU (no SPI panel) | – |
| init → rcS done | 5.7–8.0 s in QEMU | measured in emulation; does not gate the launcher |
| **power-on → launcher first frame** | **~5–6 s** (PoC estimate 6–8 s) | **estimate**, not measured on hardware |
| Pi OS M3/M4 for comparison | ~1 min 32 s | measured on hardware 2026-10-22 |

QEMU smoke test (2026-10-30, `buildroot/tests/qemu-boot.sh`, a 2 GiB SD
image with test `userconf.txt` (random password), `zpod-ssh.pub` and
`zpod-wifi.txt`): boots to userspace on both the first and the second boot.
First boot: login user `paul` created with a SHA-512 hash, key installed for
paul and root, dropbear started with `-g`, `zpod-ssh.pub`/`userconf.txt`/
`zpod-wifi.txt` deleted, the Wi-Fi network imported (hex SSID, derived PSK, no
passphrase anywhere on the image), `/data` grown from 256 MB to 1.4 GB online,
`/srv/zpod/media` bind mount, apps pre-installed, launcher/buttons
daemon/battery guard/MPD/zpod-drop running, BT modules loaded. Second boot:
grow is a no-op, nothing re-imported. As expected in QEMU there is no spidev,
I2C, UDC, wlan0 or hci0. QEMU needs a DTB copy with the PM/watchdog and
Bluetooth nodes disabled and `console=ttyAMA1` (the PL011 is ttyAMA1 on this
kernel).

Instrumentation on the device:

- `/run/zpod/boot-times` holds `<mark> <seconds since kernel start>` lines.
  It is written by `zpod-bootmark` from inittab and after each rc script, by
  the background jobs (`coldplug-done`, `audio-card`, `usb-gadget-up`,
  `wlan0-present`, `wlan0-ip`, `hci0-present`, `data-grow-done`), and by the
  launcher (`launcher-first-frame`).
- The splash logs its first frame to `/run/zpod/bootsplash.log`.
- `zpod.debug` on the kernel cmdline dumps marks, mounts, processes, accounts,
  `/data` and logs to the console after rcS and again 20 s later.
- To measure power-on time, film the screen and the power switch together and
  add the GPU stage. `/proc/uptime` starts at the kernel.

## Gaps versus the Pi OS image (M5 `v0.6.0-m5-rc1`; M6 in progress on main)

- **Not tested on hardware.** The DAC overlay/driver, the panel over spidev,
  the buttons, NCM on macOS, Wi-Fi and Bluetooth are unverified on this image.
- **RAUC A/B updates: not done** (cut for time). p3 is a stub; updates mean
  re-flashing. Plan below.
- **M6 apps:** not on main yet when this was built (Battery panel, Wi-Fi tools,
  OPDS/drop box servers). The image has the M5 apps (core 0.6.0). Merge main
  and rebuild when they land.
- **Buttons daemon is Python** (start-up costs seconds on a Zero W). Port
  `zpod_buttons.py` to Rust. gpio-keys cannot be used because GPIO20 (Vol−)
  is the I2S DIN pin.
- **No NetworkManager:** Wi-Fi networks are added only from `zpod-wifi.txt`
  (or `wpa_cli`); there is no USB "shared" profile (udhcpd serves the USB link).
- **No mDNS** (`zpod.local`): use `10.12.194.1` over USB or the Wi-Fi IP.
- **No systemd/journal:** logs in `/run/zpod/*.log`, `/data/log/*.log` and
  syslog; `systemctl` is a shim (poweroff, reboot, rsync share only).
- **`zpod-usb-kick`** (USB re-enumeration) not ported; NCM needs none on macOS.
- **MPD runs as root.** There is no `mpd` user yet.
- **Rust apps are prebuilt** (`apps/dist`, cargo-zigbuild) rather than built
  through Buildroot's cargo infrastructure.
- Time sync: BusyBox `ntpd` plus a hook that writes the DS3231.

## Next steps

1. Hardware boot on a spare card ([TEST-BR-M6.md](./TEST-BR-M6.md)). Record
   `zpod-boottime` plus a stopwatch video; fix the DAC, panel and buttons as
   needed.
2. Rewrite the buttons daemon in Rust (gpiomem polling, same timing/volume
   rules), so no Python at boot.
3. **RAUC A/B:** `BR2_PACKAGE_RAUC`, a signed bundle of the squashfs plus a
   kernel set per slot (`os_prefix=a/|b/` on p1), and a custom bootloader
   backend using firmware `tryboot` (`reboot "0 tryboot"`, `tryboot.txt`),
   mark-good from the launcher. **Verify tryboot on BCM2835 first**; the
   fallback is U-Boot with A/B env.
4. Merge the M6 apps from main and rebuild; consider a `zpod_pi0w_nexmon`
   variant pinned to a nexmon-supported kernel.
