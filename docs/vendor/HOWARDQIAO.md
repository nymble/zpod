# Howard Q. (Howard Qiao): original ZPOD software

The original uGeek / AOIDE **ZPOD** software was written and published by
**Howard Q. (Howard Qiao)**: the music player, setup scripts, the
`aoide-zpod-dac` driver packages, and the screen and input setup.

- GitHub profile: <https://github.com/howardqiao>
- Vendor sites named in his READMEs: <http://www.aoide.cc>, <http://ugeek.aliexpress.com>

This project is a new image built from scratch for the same hardware. We read
his repositories as **reference**. The hardware facts we rely on are the ones
confirmed on a real unit in [stock-introspection](../stock-introspection.md).
Where his scripts disagree with each other or with that map, the confirmed map
wins, and each conflict is noted below.

Reviewed 2026-10-22, with every public repo listed through the GitHub API.
Dates are the last push (UTC). None of his repos has been pushed since 2022.

## ZPOD-relevant repositories

| Repo | What it is | License (GitHub) | Last push | What we use or derive |
| --- | --- | --- | --- | --- |
| [zpod](https://github.com/howardqiao/zpod) | Stock ZPOD player (`play`, a closed SDL2 + libmpdclient binary), `volcontrol`, `getcover`, menu scripts, `zpod_res/` (config.txt, cmdline.txt, `aoide-zpod-dac.dtbo` and `.ko` for 4.4, `retrogame`, `fbcp`, `raspi2fb`, EmulationStation theme, shairport-sync, smb.conf) | none declared | 2020-09-17 | **Reference.** The `aoide-zpod-dac` overlay contents. Strings from the `play` binary show the RTC and DAC checks, the clock setter, and the MAX17048 SOC. `settings.ini` gives volume limits and key timing. No code is copied. |
| [aoide-dac-drivers](https://github.com/howardqiao/aoide-dac-drivers) | AOIDE DAC installer plus prebuilt `.ko`/`.dtbo` tarballs for kernels 4.4–5.10.17 (`drivers/aoide_dac_*.tar.gz`), and `aoide_zpod_setup.sh` (the full ZPOD setup) | none declared; the modules themselves say `license=GPL v2`, `author=WWW.AOIDE.CC` | 2021-03-28 | **Driver origin.** `drivers/aoide-zpod-dac/` is our GPL-2.0 rebuild of the `aoide-zpod-dac` module for 6.x kernels. Our overlay `.dts` compiles byte-identical to the `aoide-zpod-dac.dtbo` in these tarballs. Button map #1 (`aoide_zpod_setup.sh`, `/boot/retrogame.cfg`) **matches** the confirmed map pin for pin. |
| [ugeek-screen-setup](https://github.com/howardqiao/ugeek-screen-setup) | uGeek TFT setup (2.2" `pitft22`, 2.4" `pitft28-resistive`, HD-TFT DPI) with fbcp binaries | none declared | 2020-03-03 | Reference only. Its 2.2" defaults are `pitft22,speed=80000000,rotate=90,fps=60`. `overlays/ugeek-gpio-backlight.dtbo` uses **GPIO19** and is for the DPI HAT, **not** the ZPOD. |
| [getcoverart](https://github.com/howardqiao/getcoverart) | 64-line C tool that pulls the embedded picture (`AV_DISPOSITION_ATTACHED_PIC`) with libavformat | none declared | 2020-04-12 | Nothing. The Rust music player gets art from MPD's `albumart` and `readpicture`, which covers the same files without ffmpeg. |
| [AOIDE_KAZOO](https://github.com/howardqiao/AOIDE_KAZOO) | AOIDE KAZOO player (Mopidy + PiDi, ST7789) | GPL-2.0 | 2020-04-28 | One clue: `install.sh` sets `CS=0 DC=25 BL=27`, the same SPI/DC wiring as the ZPOD, with **backlight on GPIO27**. Its button maps, `ROTATE=180` and ST7789 panel are a different product. |
| [smartupsv3](https://github.com/howardqiao/smartupsv3) | uGeek Smart UPS V3 daemon (MAX17048 at 0x36 plus BQ25895 at 0x6A, WS2812 LEDs) | none declared | 2021-05-15 | Reference for MAX17048 decoding (VCELL reg 0x02 × 78.125 µV, SOC reg 0x04 / 256) and for shutdown at under 5 % SOC. The ZPOD has **no** BQ25895 (0x6A is absent). |
| [gameducky](https://github.com/howardqiao/gameducky) / [ugeek-retropie-setup](https://github.com/howardqiao/ugeek-retropie-setup) | RetroPie setup for the uGeek GameDucky (joyBonnet, `hifiberry-dac`, `pitft22 rotate=270`) | none declared | 2019-01-01 / 2018-04-09 | Different hardware, so nothing. `rotate=270` does **not** apply to the ZPOD. |
| [fbcp-ili9341](https://github.com/howardqiao/fbcp-ili9341) (fork of juj) | SPI display driver using DispmanX | MIT | 2019-10-17 | Nothing. His only change adds Pi 4 board IDs. DispmanX is gone on trixie/KMS. |
| [rpi-fbcp](https://github.com/howardqiao/rpi-fbcp) (fork of tasanakorn) | Framebuffer mirror | MIT | 2020-02-24 | Nothing. Identical to upstream. |

## Other public repositories (not ZPOD)

`myvolumio` (GitHub returns HTTP 451: DMCA block of 2024-06-12, Volumio), `RaspiVoiceHAT` (WM8960 HAT, binary drivers), `ugeek-ups2-setup` (BQ27441 UPS),
`ugeek-rmp-setup` (Raspi Media Player DPI screen), `retrogame-mcp23008` (Adafruit retrogame, BSD), `apploader` (SDL launcher, 800×480),
`raspi_motohat`, `raspihdtftplus` (MIT), and forks `Build`, `Volumio2`, `Volumio2-UI`, `volumio-plugins`,
`rpi-backup`, `bkup_rpimage`, `rpi-update`, `BuffaloDAC`, `hyperpixel`, `EmulationStation`, `PiClock`,
`i2c-keypad-library-raspberrypi`, `rtl8723bu`. That is 31 public repos in total.

## Driver licensing

- The vendor `aoide-zpod-dac.ko` (4.4.13 in `zpod/zpod_res`, 4.9–5.10.17 in
  `aoide-dac-drivers/drivers/`) declares `MODULE_LICENSE("GPL v2")`,
  `MODULE_AUTHOR("WWW.AOIDE.CC")`, description "ASoC Driver for Aoide Zpod DAC".
- No C source for it is published in any of his repos, and GitHub code search
  turns up none.
- Our `drivers/aoide-zpod-dac/aoide-zpod-dac.c` is a GPL-2.0 out-of-tree
  rebuild against the same overlay `compatible = "aoide,aoide-zpod-dac"` and
  codec `pcm5122@4d`. It carries the original vendor attribution in its header.
- `zpod/zpod_res/aoide-zpod-dac.dtbo` (sha256 `1eeb2184…`) and the
  tarball/stock `aoide-zpod-dac.dtbo` (sha256 `3ad223aa…`) decompile to the
  same tree. The only difference is the unresolved-phandle placeholder
  (`0xdeadbeef` vs `0xffffffff`). Our `.dts` reproduces the `3ad223aa…` file.

## Findings vs our confirmed hardware facts

| Topic | His files say | Our confirmed fact | Verdict |
| --- | --- | --- | --- |
| Buttons | `aoide_zpod_setup.sh` retrogame: L17 R23 U4 D22, U=12 I=13 J=16 K=26, EQUAL=5 MINUS=20, RIGHTSHIFT=24 (Home), ENTER=6 (Play) | same pins | **match** |
| Buttons | `zpod/zpod_res/retrogame.cfg` (K5, LEFT22, RIGHTSHIFT23, RIGHT24), `aoidetft_setup.sh`, KAZOO maps | — | **conflict**: PiTFT/KAZOO samples, ignore |
| D-pad centre | no pin in any map; vendor ENTER is GPIO6 (Play) | centre = 4+17+22+23 together | no new info |
| Panel | `pitft22` → fbtft `fb_ili9340`, DC 25, `bgr`, `rotate=90` | ILI9340-style, DC 25, MADCTL 0xE8 | **match**: fbtft rotate 90 = MY\|MX\|MV, plus BGR = 0xE8 |
| SPI clock | 64 MHz (`zpod_res/config.txt`, fps 30) or 80 MHz (`aoide_zpod_setup.sh`, fps 60) | we run 32 MHz | not a conflict; there may be headroom (test) |
| Panel rotation | `rotate=270` (gameducky), `ROTATE=180` (KAZOO) | 0xE8 | **conflict**: other products |
| GPIO27 | KAZOO `BL=27`; vendor `play`/`volcontrol` set GPIO27 high once | held high, inferred backlight | **supports** backlight (still unproven on ZPOD) |
| GPIO14 | `volcontrol` drives wiringPi 15 (= BCM14) | amp mute, low = unmuted | **match** |
| DAC | `i2cdetect` row `40` column `d` check ("DAC OK!") | PCM5122 at 0x4d | **match** |
| RTC | `i2cdetect` row `60` column `8` ("RTC OK!"); clock menu runs `date -s` and `hwclock -w` | DS3231 at 0x68, overlay not enabled | **match**; vendor wrote the RTC from the clock screen |
| Fuel gauge | `max17048_getsoc` in `play`; smartups VCELL/SOC math | MAX17048 at 0x36 | **match**. Do not copy smartups' `write_word(0xFE, 0xFFFF)` "init": 0xFE is the command register, and the documented reset value is 0x5400 |
| IR | `dtoverlay=gpio-ir,gpio_pin=7` in `zpod_res/config.txt` | unverified (GPIO7 = SPI0 CE1) | open |
| Volume | MPD mixer `hw:0` `Digital`; `volume_limit=80`, `volume_step=2` | ALSA `Digital`, driver caps at 0 dB (207) | compatible |
| Overlay config | `zpod_res/config.txt` has `dtparam=spi=off` and also `pitft22` (which enables spi0 itself) | `dtparam=spi=on` | inconsistency inside his files; harmless |

## Worth adopting (candidates, not yet done)

1. **DS3231 RTC.** Add `dtoverlay=i2c-rtc,ds3231` and drop `fake-hwclock`. Let the clock app set the time and write the RTC (`hwclock -w`), the way the vendor clock menu did. Small effort; needs a hardware check.
2. **GPIO27 = backlight.** KAZOO `BL=27` on the same CS0/DC25 wiring makes this likely. Prove it with TEST-M3 4.3, then describe it as `gpio-backlight` (or keep it in userspace). Small effort.
3. **SPI clock.** The vendor ran the panel at 64–80 MHz; we run 32 MHz. Try 48 and 64 MHz for faster full-frame redraws (album art). Small effort; needs a hardware check.
4. **Kernel DRM panel (option).** `dtoverlay=pitft22,drm` binds the in-tree `ili9341` tiny-DRM driver (`adafruit,yx240qv29`). Its `rotation=270` gives MADCTL 0xE8. This only matters if apps should draw through KMS instead of spidev. Medium effort.
5. **Low-battery shutdown** at under 5 % SOC (smartups behaviour) and a 6-step battery icon (vendor `play`). Small effort.
6. **Volume and key timing defaults** from `zpod/settings.ini`: cap 80 %, step 2, short press 75–650 ms, long press up to 1200 ms. Useful for long-press actions in the Rust apps. Small effort.
7. **Emulation keymap** from `aoide_zpod_setup.sh` `config_retroarch`: a=K (Circle), b=J (X), x=I (Triangle), y=U (Square), hotkey=Home, exit=Play, menu=Triangle, state slot = left/right. It already lines up with `zpod-buttons` (U/I/J/K). Useful for M9.
8. **AirPlay receiver** (optional). The vendor shipped `shairport-sync` named "ZPOD" on `hw:0,0`. Debian trixie packages it. Medium effort.

Not adopted: the vendor access-point mode (it hard-codes a default WPA
passphrase), the ffmpeg cover extraction (MPD already does this), the vendor
`.ko`/`.dtbo` binaries, and fbcp/raspi2fb (we drive the panel directly).
