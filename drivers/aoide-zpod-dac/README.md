# aoide-zpod-dac (DKMS)

ASoC machine driver + device-tree overlay for the ZPOD's TI **PCM5122** DAC
(I2C1 @ `0x4d`, I2S). Confirmed working on a Pi Zero W running
`6.18.50+rpt-rpi-v6` on 2026-10-07 (card `sndrpiaoidezpod`).

| File | Purpose |
| --- | --- |
| `aoide-zpod-dac.c` | GPL-2.0 out-of-tree rebuild of the vendor `aoide-zpod-dac` module for 6.x kernels (vendor `.ko` only existed for 4.x/5.10) |
| `aoide-zpod-dac-overlay.dts` | Overlay source. Compiled with `dtc -@` it is **byte-identical** to the vendor `zpod_res/aoide-zpod-dac.dtbo` (sha256 `3ad223aa…c069`) |
| `dkms.conf`, `Makefile` | DKMS build (rebuilds automatically on kernel upgrades) |
| `build-deb.sh` | Produces `aoide-zpod-dac-dkms_1.0.1-1_all.deb` (source in `/usr/src`, overlay in `/boot/firmware/overlays/`) |

The M2 image installs the deb in the pi-gen chroot with
`linux-headers-rpi-v6` and `linux-headers-rpi-v7`, so the module is built for
both 32-bit kernel flavours. The arm64 `rpi-v8` kernel shipped in the armhf
image is **not** covered (no arm64 toolchain in an armhf userland); the Zero W
never boots it.

Origin: the vendor module (GPL v2, author `WWW.AOIDE.CC`) and its `.dtbo` were published as prebuilt binaries by Howard Q. (Howard Qiao) in
[howardqiao/aoide-dac-drivers](https://github.com/howardqiao/aoide-dac-drivers) and
[howardqiao/zpod](https://github.com/howardqiao/zpod). No vendor C source is public, so this is a
rebuild. See [docs/vendor/HOWARDQIAO.md](../../docs/vendor/HOWARDQIAO.md).

Required `config.txt` lines (the image sets these):

```
dtparam=i2c_arm=on
dtparam=i2s=on
dtparam=audio=off
dtoverlay=aoide-zpod-dac
gpio=14=op,dl     # headphone amp un-mute (GPIO14 LOW)
```

and `console=serial0,115200` removed from `cmdline.txt` (GPIO14 is UART TX).

## Red "play" LED / amp enable (1.0.1, 2026-11-01)

On hardware (Buildroot test2) the red LED next to Play stayed lit with
nothing playing. The LED follows the PCM5122's **GPIO4**, which this driver
uses as the amp enable (register page 0: `GPIO_EN` 0x08 bit 3 = GPIO4 is an
output, `GPIO_OUTPUT_4` 0x53 = 0x02 = driven from the register,
`GPIO_CONTROL_1` 0x56 bit 3 = the level).

Cause found in the source: the card `init` callback (run once when the card
registers at boot) set `GPIO_CONTROL_1` bit 3 **high**, copied from the
vendor module. `startup()` (a PCM stream opens) also sets it high and
`shutdown()` (the stream closes) clears it, so the LED was on from boot until
the first stream had opened and closed. Nothing else at boot opens a PCM:
`zpod-audio-init` and `alsactl` only touch mixer controls, MPD opens the
output only when it plays (`restore_paused` keeps a paused state closed), and
the 440 Hz tone runs only from Settings > Diagnostics.

1.0.1 leaves GPIO4 **low** at init, so the amp (and LED) is on only while a
stream is open. While MPD is *paused* it may keep the device open, so the LED
can stay on during a pause; it goes off on stop. **Not yet confirmed on
hardware** (QEMU has no I2C DAC): check that the LED is off after boot, on
while music plays, and off again after Stop.
