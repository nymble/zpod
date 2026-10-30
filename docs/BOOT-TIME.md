# Boot time (M6)

M5 (`v0.6.0-m5-rc1`) takes **1:09** from power-on to the menu on Paul's ZPOD
(measured on hardware). M6 (`v0.9.0-m6-rc1`) trims the Pi OS boot. **The M6
number below is an estimate from QEMU; it has not been measured on the
ZPOD yet** (TEST-M6.md section 0 asks for it).

## What M6 changes (build/tweaks/stage2-08-zpod-m6)

| Change | Why it is safe here |
|---|---|
| `auto_initramfs=0` (no 11 MB initramfs) | the rpi-v6 kernel has ext4, mmc_block and both SD host drivers built in (`modules.builtin`, checked at build time, build fails otherwise); `root=PARTUUID=…` needs no udev |
| `dtoverlay=vc4-kms-v3d` and `max_framebuffers` commented out | the LCD is SPI driven from userspace; no HDMI use. **HDMI output is gone** |
| `camera_auto_detect=0`, `display_auto_detect=0` | no camera or DSI display |
| `disable_splash=1`, `boot_delay=0`, `initial_turbo=30` | firmware-side; full clock for the first 30 s (no overvolt) |
| cmdline `quiet loglevel=3 logo.nologo` | less text on the framebuffer console |
| `keyboard-setup`, `rpi-eeprom-update` masked; `apparmor`, `e2scrub_reap` disabled | no keyboard, no EEPROM on a Zero, no AppArmor profiles used, no LVM |
| Python `zpod-battery-guard` → Rust `zpod-battery` | one less Python interpreter at boot (and every 30 s after) |

Kept on purpose: SSH over USB (`zpod-usb-gadget`, `rpi-usb-gadget-ics`,
`sshswitch`, host-key regeneration), first-boot files (`userconfig`,
`zpod-firstuser`, `zpod-wifi-import`), `/data` grow + setup, avahi,
Bluetooth, NetworkManager. The verifier checks each of these is still enabled.

## QEMU comparison (estimate)

`qemu-system-arm -M raspi0` (QEMU 10.0, TCG inside the colima `zpod` VM),
each image's own `kernel.img` + `bcm2708-rpi-zero-w.dtb` (watchdog and
Bluetooth nodes disabled for QEMU; M5 also with its initramfs and the
vc4-kms-v3d overlay applied, as its firmware would), scratch copies only,
both images booted at the same time so they shared the same host load.
Times are from each image's own persistent journal (monotonic seconds).

| Steady-state boot (3rd for M5, 2nd/3rd for M6) | M5 | M6 |
|---|---|---|
| kernel (systemd says) | 13.4 s | 3.8 / 7.5 s |
| sysinit.target | 52.4 s | 42.0 / 44.6 s |
| **zpod-launcher started** | **93.1 s** | **81.6 / 82.8 s** |
| ssh started | 98.4 s | 86.4 / 87.9 s |

QEMU here runs slower than the real Zero and has no DAC, SPI, Wi-Fi or USB
gadget, so only the ratio means anything: launcher start **−11 to −12 %**.
Scaled to the measured 1:09 that is about **61 s**, plus the firmware-only
savings QEMU cannot show (initramfs read from SD, splash, boot delay,
initial turbo), so **estimate ≈ 55-62 s on hardware**. The 35-45 s target is
**not** reached by these trims alone.

## Next trims (not in M6, need a hardware check first)

1. `zpod-launcher` waits for `zpod-audio-init` (`After=`); in QEMU (no DAC)
   that wait was the longest gap before the menu. The menu does not need the
   mixer: drop the ordering (keep it for the alarm path).
2. `zpod-buttons` is Python; a Rust version would start in a fraction.
3. NetworkManager and the USB gadget are before `network.target`; the
   launcher does not need the network to draw the menu.
4. `systemd-analyze critical-chain zpod-launcher.service` on the ZPOD
   (TEST-M6.md) will show which of these matter on real hardware.
