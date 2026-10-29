# Buildroot fast-boot image (proof of concept)

**Status: experimental, built but not yet booted on hardware (2026-10-29).**
Tracking issue: [#BR] (see the issue linked from the PR). Branch: `buildroot`.

This is the second OS track for the ZPOD: a Buildroot image with BusyBox
init, a read-only squashfs root and a writable `/data` partition, running the
same Rust launcher and apps as the pi-gen image. The goal is **power-on to
launcher in about 4–7 s** (estimate) instead of about 1.5 min today (pi-gen
M3/M4, measured on hardware 2026-10-22: menu after about 1 min 32 s).

The pi-gen image stays the supported image. Nothing here changes it.

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
| `configs/zpod_pi0w_defconfig` | ARM1176JZF-S, internal glibc toolchain (GCC 15, C++, headers from the 6.12 kernel; Bootlin toolchains are x86_64-host only), RPi kernel 6.12.61 `bcmrpi`, rpi-firmware, brcmfmac firmware, BusyBox init + mdev, squashfs LZ4 root. Packages: ALSA, MPD (alsa, flac, mpg123, faad2, vorbis, opus, id3tag, sqlite), python3 + evdev + smbus2, wpa_supplicant, iw, dropbear, gesftpserver (macOS `scp` uses SFTP), rsync, i2c-tools, e2fsprogs, exfatprogs |
| `board/zpod/linux.fragment` | squashfs/ext4 built in (no initramfs), spidev + GPIO cdev, uinput, `RASPBERRYPI_GPIOMEM=y`, bcm2835 I2S + PCM512x, I2C chardev, **DS3231 built in with `RTC_HCTOSYS`** (clock is right before userspace), dwc2 + configfs NCM/ECM/RNDIS/ACM/mass storage, brcmfmac (module), exFAT |
| `board/zpod/config.txt` | the pi-gen hardware lines (I2C/I2S/SPI, `gpio=14=op,dl`, DC/backlight pins, button pull-ups, `dtoverlay=aoide-zpod-dac`, `i2c-rtc,ds3231`, `dwc2,dr_mode=peripheral`), plus `hdmi_ignore_hotplug=1`, `disable_splash=1`, `boot_delay=0`, `auto_initramfs=0`, `gpu_mem=16` |
| `board/zpod/cmdline.txt` | `root=/dev/mmcblk0p2 rootfstype=squashfs ro quiet loglevel=3 console=tty1` (no serial console: GPIO14 must stay low) |
| `package/zpod-dac` | `aoide-zpod-dac.ko` built from `drivers/aoide-zpod-dac/` (same source as the DKMS package) |
| `package/zpod-overlays` | `aoide-zpod-dac.dtbo` compiled from `drivers/aoide-zpod-dac/aoide-zpod-dac-overlay.dts` |
| `package/zpod-apps` | installs `apps/dist` (static musl ARMv6 launcher, bootsplash, drop, zpod-app + music/clock/tetris `.zpa`) |
| `package/zpod-scripts` | `overlay/usr/lib/zpod/{zpod_buttons.py, zpod_battery_guard.py, zpod_hw.py, zpod-usb-gadget.sh, zpod-audio-init.sh}`, `asound.conf`, `buttons.conf`, `ui.conf` from the pi-gen overlay (single source) |
| `board/zpod/rootfs-overlay/` | inittab, rcS with boot marks, init scripts, first-boot Wi-Fi/SSH import, mpd.conf, udhcpd, `systemctl` shim (poweroff/reboot only) |
| `board/zpod/post-build.sh` | read-only root plumbing: `/opt/zpod/apps → /data/zpod/apps`, `/srv/zpod/media → /data/media`, `/var/lib/{mpd,zpod,alsa,seedrng} → /data/var/*`, `/root/.ssh → /data/etc/ssh`, mdev rules for the `zpod` user, `/etc/zpod-release` |
| `board/zpod/post-image.sh`, `genimage.cfg.in` | pre-installs the bundled apps into the data partition and assembles `sdcard.img` |

### Partition layout (MBR)

| # | Label | FS | Size | Mounted | Contents |
|---|---|---|---|---|---|
| p1 | ZPODBOOT | FAT32 | 64 MB | `/boot/firmware` (rw, 0700) | firmware, `zImage`, DTB, overlays, `config.txt`, `cmdline.txt`; drop-ins `zpod-wifi.txt`, `zpod-ssh.pub`, `zpod-usb.txt` |
| p2 | rootA | squashfs LZ4 | 256 MB partition (≈47 MB image) | `/` read-only | the OS |
| p3 | rootB | squashfs | 256 MB | – | **A/B stub**: same image as A, never booted yet |
| p4 | zpoddata | ext4 | 1 GB (grow-to-card is a next step) | `/data` | music, playlists, apps, Wi-Fi config, SSH keys, dropbear host keys, MPD/ALSA state |

`sdcard.img` is 1.6 GB (1.56 GiB). Writable state lives only on `/data` and
tmpfs (`/run`, `/tmp`, `/var/log`).

### Boot sequence (BusyBox init)

1. `sysinit`: mount proc, sys and `/run`. Start **`zpod-bootsplash`** (Rust,
   spidev). Run `mount -a` (`/data`, `/boot/firmware`). Start the
   **launcher** in the background (`zpod-launcher-early`).
2. `rcS` runs `S00zpod-data` (layout, device permissions, first-boot
   imports), seedrng, syslogd/klogd, `S10mdev` (module coldplug **in the
   background**), `S30zpod-hw` (buttons daemon, audio defaults, battery
   guard; background), `S35usbgadget` (CDC-NCM + udhcpd; background),
   `S40wifi` (background), `S60zpod-drop`. Dropbear and MPD start in the
   background.
3. `respawn`: `zpod-launcher-run` waits for the early launcher and restarts it
   whenever it exits. `tty1` getty is for HDMI debugging.
4. `shutdown`: `zpod-launcher-stop` sends SIGTERM (end screen), then `rcK`.

The launcher retries the input device every 2 s. So it can draw before the
python buttons daemon is up; the buttons work about 1–3 s later (estimate).

## First boot (no credentials in the image)

After writing the image to a **disposable** card (Raspberry Pi Imager "Use
custom", or `dd`; this repo never flashes cards), the FAT volume mounts on the
Mac as `/Volumes/ZPODBOOT`.

Step 1, SSH key (key-only login; passwords are disabled):

```zsh
cp ~/.ssh/id_ed25519.pub /Volumes/ZPODBOOT/zpod-ssh.pub
```

Step 2, Wi-Fi (optional; replace the placeholders, keep the single quotes):

```zsh
printf 'SSID=%s\nPSK=%s\n' '<SSID>' '<PW>' > /Volumes/ZPODBOOT/zpod-wifi.txt
```

Step 3:

```zsh
diskutil eject /Volumes/ZPODBOOT
```

On first boot, `S00zpod-data` appends the key to `/data/etc/ssh/authorized_keys`
and deletes `zpod-ssh.pub`. It writes the network to
`/data/etc/wpa_supplicant.conf` (0600; SSID hex-encoded, only the derived WPA
PSK is stored, never the passphrase) and then shreds and deletes
`zpod-wifi.txt`. The file format is the same as for pi-gen
([BOOT-WIFI.md](./BOOT-WIFI.md)).

Log in over USB (CDC-NCM, Mac gets 10.12.194.2–14 by DHCP) or Wi-Fi:

```zsh
ssh root@10.12.194.1
zpod-boottime        # this boot's timing marks
```

Only `root` has SSH. The `zpod` user (uid 1000) runs the apps and zpod-drop.

## Boot time

| Stage | Number | Kind |
|---|---|---|
| GPU firmware (bootcode.bin → start.elf → kernel load), with `hdmi_ignore_hotplug` | ~2.3–2.5 s | estimate (ahepp.dev, Zero W) |
| zImage load: 9.4 MB at ~6 MiB/s | ~1.5 s | estimate |
| kernel start → `init` | 2.5 s in QEMU `raspi0` (TCG) | **measured in emulation**, real HW expected ~1.5–2.5 s with this untrimmed kernel |
| init → launcher exec | 0.33 s in QEMU | **measured in emulation** |
| init → rcS done (all background jobs started) | 5.7 s in QEMU | **measured in emulation**; it does not gate the launcher |
| launcher exec → first frame | not measurable in QEMU (no SPI panel) | – |
| **power-on → launcher first frame** | **~6–8 s** with this kernel; **~4–6 s** after kernel trimming | **estimate** |
| pi-gen M3/M4 for comparison | ~1 min 32 s | measured on hardware 2026-10-22 |

QEMU smoke test (2026-10-29, `qemu-system-arm -M raspi0`, the PM/watchdog
node disabled in a copy of the DTB because QEMU does not emulate it): the
image boots to userspace. `/` is squashfs ro, `/data` ext4 rw and
`/boot/firmware` vfat. The apps are pre-installed in `/data/zpod/apps`. The
buttons daemon (uinput `zpod-buttons`), battery guard, dropbear, MPD,
zpod-drop and the launcher all run. As expected in QEMU, there is no spidev,
no `/dev/i2c-1`, no UDC and no wlan0 (no firmware, so no overlays). Marks from
`/run/zpod/boot-times` in QEMU: init-start 2.57, launcher-exec 2.90, rcS-done
8.28, dropbear 10.7, mpd 11.8, coldplug 15.4 s.

Instrumentation on the device:

- `/run/zpod/boot-times` holds `<mark> <seconds since kernel start>` lines.
  It is written by `zpod-bootmark` from inittab and after each rc script, by
  the background jobs (`coldplug-done`, `audio-card`, `usb-gadget-up`,
  `wlan0-present`, `wlan0-ip`), and by the launcher (`launcher-first-frame`,
  new in `apps/crates/launcher`).
- The splash logs its first frame to `/run/zpod/bootsplash.log`.
- `zpod.debug` on the kernel cmdline dumps marks, mounts, processes and logs
  to the console after rcS and again 20 s later.
- To measure power-on time, film the screen and the power switch together and
  add the GPU stage. `/proc/uptime` starts at the kernel.

## Gaps versus the pi-gen image (v0.6.0-m5-rc1)

- **Not tested on hardware.** The DAC overlay/driver, the panel over spidev,
  the buttons, NCM on macOS and Wi-Fi are all unverified on this image.
- **Kernel not trimmed** (zImage 9.4 MB, `bcmrpi_defconfig` + fragment;
  `KALLSYMS` is still selected). Target ≤ 3.5 MB.
- **Buttons daemon is Python** (start-up costs seconds on a Zero W). Port
  `zpod_buttons.py` to Rust. gpio-keys cannot be used because GPIO20 (Vol−)
  is the I2S DIN pin.
- **No Bluetooth stack** (BlueZ not included): the M5 BT scanner will not work.
- **Wi-Fi scanner/info:** no NetworkManager. `zpod-hal` now falls back to
  `iw dev wlan0 link` for SSID/signal; scanner features that use `nmcli`
  need an `iw`/`wpa_cli` backend.
- **M5 extras not ported:** `zpod-rsyncd` (rsync daemon socket),
  `zpod-data-grow`/`zpod-data-setup` (here `/data` is a fixed 1 GB p4),
  `zpod-usb-kick`, `zpod-firstuser` (here: root key-only), `userconf.txt`.
- **MPD runs as root.** There is no `mpd` user yet.
- **A/B is a stub:** p3 holds the same squashfs and nothing switches slots.
- **Rust apps are prebuilt** (`apps/dist`, cargo-zigbuild) rather than built
  through Buildroot's cargo infrastructure.
- Time sync: BusyBox `ntpd` plus a hook that writes the DS3231. There is no
  "set time" integration test.

## Next steps

1. Hardware boot on a spare card. Record `zpod-boottime` plus a stopwatch
   video; fix the DAC, panel and buttons as needed.
2. Trim the kernel (`localmodconfig`-style from the running module list,
   drop DRM/V4L/media/sound drivers that are not used, `KALLSYMS=n`, LZ4) and
   build the needed drivers in. Measure again.
3. Rewrite the buttons daemon in Rust (gpiomem polling, same timing/volume
   rules), so no Python at boot.
4. **RAUC A/B:** `BR2_PACKAGE_RAUC`, a signed bundle of the squashfs plus a
   kernel set per slot (`os_prefix=a/|b/` on p1), and a custom bootloader
   backend using firmware `tryboot` (`reboot "0 tryboot"`, `tryboot.txt`),
   mark-good from the launcher. **Verify tryboot on BCM2835 first**; the
   fallback is U-Boot with A/B env.
5. **Read-only root hardening:** keep the root ro (already), move
   `/etc/dropbear` and alsa state fully onto `/data`, run `fsck` on `/data` in
   the background, grow `/data` to the card on first boot.
6. Bring the M5 services across (rsyncd, BT via BlueZ started lazily, Wi-Fi
   scanner backend) and a `zpod_pi0w_nexmon` variant pinned to a
   nexmon-supported kernel.
