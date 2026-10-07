# aoide-zpod-dac (DKMS)

ASoC machine driver + device-tree overlay for the ZPOD's TI **PCM5122** DAC
(I2C1 @ `0x4d`, I2S). Confirmed working on a Pi Zero W running
`6.18.50+rpt-rpi-v6` on 2026-10-07 (card `sndrpiaoidezpod`).

| File | Purpose |
| --- | --- |
| `aoide-zpod-dac.c` | GPL-2.0 out-of-tree rebuild of the vendor `aoide-zpod-dac` module for 6.x kernels (vendor `.ko` only existed for 4.x/5.10) |
| `aoide-zpod-dac-overlay.dts` | Overlay source. Compiled with `dtc -@` it is **byte-identical** to the vendor `zpod_res/aoide-zpod-dac.dtbo` (sha256 `3ad223aa…c069`) |
| `dkms.conf`, `Makefile` | DKMS build (rebuilds automatically on kernel upgrades) |
| `build-deb.sh` | Produces `aoide-zpod-dac-dkms_1.0.0-1_all.deb` (source in `/usr/src`, overlay in `/boot/firmware/overlays/`) |

The M2 image installs the deb in the pi-gen chroot with
`linux-headers-rpi-v6` and `linux-headers-rpi-v7`, so the module is built for
both 32-bit kernel flavours. The arm64 `rpi-v8` kernel shipped in the armhf
image is **not** covered (no arm64 toolchain in an armhf userland); the Zero W
never boots it.

Required `config.txt` lines (the image sets these):

```
dtparam=i2c_arm=on
dtparam=i2s=on
dtparam=audio=off
dtoverlay=aoide-zpod-dac
gpio=14=op,dl     # headphone amp un-mute (GPIO14 LOW)
```

and `console=serial0,115200` removed from `cmdline.txt` (GPIO14 is UART TX).
