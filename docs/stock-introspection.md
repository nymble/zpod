# Stock / working-image introspection

Status: **live probe 2026-10-07 PT** on Paul's booted M1.1 card (Pi Zero W). Display, buttons, audio and I2C devices are now **confirmed on hardware** (see the next section; shipped in M2 `v0.3.0-m2`). Vendor/online sections further down remain non-authoritative where they conflict with live facts.

## Confirmed hardware map (live Pi Zero W, 2026-10-07 PT)

All GPIO numbers are **BCM**. Confirmed by live probes on Paul's ZPOD
(`zdisp.py`, `zorient.py`, `btnlog2.py`, `speaker-test`, `i2cdetect`).

### Audio (Aoide ZPOD DAC)

| Fact | Value |
| --- | --- |
| Codec | TI **PCM5122** on **I2C1 @ 0x4d** (`UU` once bound) |
| config.txt | `dtparam=i2c_arm=on`, `dtparam=i2s=on`, `dtparam=audio=off`, `dtoverlay=aoide-zpod-dac` |
| Overlay | vendor `aoide-zpod-dac.dtbo` (howardqiao/zpod `zpod_res`); our `drivers/aoide-zpod-dac/aoide-zpod-dac-overlay.dts` compiles byte-identical |
| Driver | GPL-2.0 OOT `aoide-zpod-dac.ko` rebuilt for 6.18 (M2 ships it via DKMS) |
| ALSA card | `sndrpiaoidezpod` / `snd_rpi_aoide_zpod_dac`; mixer `Digital`, `Analogue`, `Analogue Playback Boost`, … |
| Headphone amp | **muted unless GPIO14 is LOW** → `gpio=14=op,dl` |
| Serial console | must be removed from `cmdline.txt` (`console=serial0,115200`): GPIO14 is UART TX |
| I2S pins | GPIO18 PCM_CLK, 19 PCM_FS, 20 PCM_DIN, 21 PCM_DOUT (alt0) |

### Display (works on hardware)

| Fact | Value |
| --- | --- |
| Controller | ILI9340-style command set (SWRESET, SLPOUT, COLMOD 0x55, MADCTL, INVOFF, NORON, DISPON) |
| Bus | `spidev0.0`: SPI0 **CE0 = GPIO8**, **MOSI GPIO10**, **SCLK GPIO11**, no MISO, 32 MHz, mode 0 |
| DC | **GPIO25** |
| Reset | none (software reset 0x01) |
| GPIO27 | held **HIGH** — probably backlight or power enable (*inferred*, not proven) |
| Geometry | **320×240 landscape**, MADCTL `0x36` = **`0xE8`** (MV\|MX\|MY\|BGR), column 0–319, page 0–239 |
| Init order | SWRESET, 150 ms, SLPOUT, 150 ms, COLMOD 0x55, MADCTL 0x08, INVOFF, NORON, DISPON, then MADCTL 0xE8 |

This replaces the "ST7789 1.3\" 240×240" online claim and fixes the vendor `pitft22` 320×240 note: the panel is 320×240 on SPI0 CE0 with DC 25.

### Buttons (active-low, internal pull-ups)

| Button | GPIO |
| --- | --- |
| D-pad up | 4 |
| D-pad down | 22 |
| D-pad left | 17 |
| D-pad right | 23 |
| D-pad centre | *no pin*: GPIO 4, 17, 22, 23 all go low within ~40 ms |
| Home | 24 |
| Play/Pause | 6 |
| Triangle | 13 |
| Square | 12 |
| Circle | 26 |
| X | 16 |
| Side volume up | 5 |
| Side volume down | 20 (**also I2S DIN** — set pull-up only, read via `/dev/gpiomem`/level register, never re-mux) |

Matches vendor map #1 below (`aoide_zpod_setup.sh` retrogame.cfg) pin-for-pin; the other vendor maps are wrong for this unit.

### Other I2C1 devices

| Address | Device |
| --- | --- |
| 0x36 | **MAX17048** fuel gauge (SOC reg 0x04, VCELL reg 0x02) |
| 0x4d | PCM5122 DAC |
| 0x68 | **DS3231** RTC (time not set; no RTC overlay enabled yet) |

### Wi-Fi

Dropouts observed inside the metal case with brcmfmac power save on → M2 disables power save (NetworkManager `wifi.powersave = 2`).


Purpose: record what the current distribution actually contains so new requirements are evidence-based, without forcing the first new build to clone 2017 feature-for-feature.

## Capture checklist (run on a booted ZPOD or from a mounted image)

- [x] `uname -a`, `/etc/os-release`, kernel package version
- [~] `/boot/config.txt` stock M1.1 summary (full paste still welcome); `/boot/cmdline.txt` not yet captured
- [x] overlays present include `pitft22`, `minipitft13`, many pcm512x DACs; stock lacked `aoide-zpod-dac.dtbo` — **now installed live** on Paul's M1.1 (see DAC enable section)
- [x] `aplay -l`, `i2cdetect -y 1` (after enabling i2c); DAC overlay enabled live — see below; `lsmod` still welcome
- [~] package count ~520; `i2c-tools`, `python3-spidev`, `python3-rpi-lgpio` installed — full `dpkg -l` artifact still welcome
- [ ] systemd units / init scripts that start the UI, player, RetroPie, or button daemon
- [ ] Button daemon config path and GPIO map file, if any
- [ ] Network: `iw list`, `hciconfig` / `bluetoothctl show`, USB Wi-Fi dongle VID:PID if present
- [ ] Partition layout (`lsblk -f`) and media mount points

## Where results go

- Attach logs to issue **[INTROSPECT]**.
- Summarize durable facts into `config/hardware.yaml` and this file via PR.
- Turn each unexpected package or missing overlay into a linked sub-issue; do not silently expand scope.


## Live probe 2026-10-07

**Source:** Paul flashed **M1.1** `v0.2.0-m1.1-wifi`, SSH as user `paul` @ `zpod.local` / `192.168.1.81`. Probe while uptime ~18 min. Auth: nymble. **No credentials in this doc.**

| Fact | Value |
| --- | --- |
| Model | Raspberry Pi **Zero W Rev 1.1** (not Zero 2 W) |
| Kernel | `Linux zpod 6.18.50+rpt-rpi-v6 #1 Raspbian 1:6.18.50-1+rpt1 armv6l` |
| OS | Raspbian **13 trixie** |
| Wi-Fi | `wlan0` connected; NetworkManager connection name `"preconfigured"` |
| Packages | ~520 dpkg packages; `i2c-tools`, `python3-spidev`, `python3-rpi-lgpio` installed |

### `config.txt` (stock M1.1 at probe)

- i2c / spi / i2s **commented out** initially
- `dtparam=audio=on`
- `vc4-kms-v3d`
- **No** TFT or DAC overlays loaded

### Buses after enabling I2C

Ran `raspi-config nonint do_i2c 0`, then `i2cdetect -y 1`:

| Address | Note |
| --- | --- |
| **0x36** | present (identity not claimed here) |
| **0x4d** | matches **PCM5122 / `aoide-zpod-dac` expectation** ([#4](https://github.com/nymble/zpod/issues/4)) |
| **0x68** | present (identity not claimed here) |

`aplay -l` (stock M1.1): only **vc4hdmi** — DAC overlay not loaded yet.

### Live DAC enable 2026-10-07 PT (M2 audio bring-up)

After installing vendor `aoide-zpod-dac.dtbo` + GPL OOT `aoide-zpod-dac.ko` rebuilt for `6.18.50+rpt-rpi-v6` (stock vendor `.ko` is 4.4/5.10-only):

| Fact | Value |
| --- | --- |
| Config | `dtparam=i2c_arm=on`, `dtparam=i2s=on`, `dtparam=audio=off`, `dtoverlay=aoide-zpod-dac` (backup `config.txt.bak.pre-aoide-*`) |
| `i2cdetect -y 1` | **`UU` at 0x4d** (driver bound; was `4d` before overlay) |
| `aplay -l` | **card 1: `sndrpiaoidezpod` / `snd_rpi_aoide_zpod_dac`**, device `Aoide Zpod DAC HiFi pcm512x-hifi-0` (card 0 still vc4hdmi) |
| Test tone | `speaker-test -D plughw:1,0 -c 2 -r 44100 -t sine -f 440` (and 523 Hz) **exit 0** — Paul must confirm headphone hear |

Module provenance: OOT rebuild under `~/aoide-zpod-dac-oot` on device; compatible `aoide,aoide-zpod-dac`; uses stock `snd-soc-pcm512x`. No TFT/button pins invented. Auth: nymble. **No credentials in this doc.**

`gpioinfo` (partial):

- `spi0` CS0=GPIO8, CS1=GPIO7 already claimed
- GPIO2/3 consumer=`kernel` (I2C)

### Overlays on M1.1 stock firmware

Live note + matching pi-gen stage2 rootfs (`boot/firmware/overlays`, 386 entries):

- Present: `pitft22`, `minipitft13`, many `pcm512x` / HiFiBerry / Allo DAC overlays
- **Absent: `aoide-zpod-dac.dtbo`** — must be added from vendor (`howardqiao/zpod` `zpod_res/` or `aoide-dac-drivers` tarball) or rebuilt for this kernel before M2 audio enable

### Still not claimed (do not invent)

- ~~Button BCM map~~ — confirmed, see top section
- ~~TFT DC / CS pinout~~ — confirmed (CE0, DC25); GPIO27 role still *inferred* (backlight/power)
- ~~Identity of I2C `0x36` / `0x68`~~ — MAX17048 / DS3231
- IR receiver pin (vendor says `gpio-ir` on GPIO7; not verified, and GPIO7 is SPI0 CE1)
- Full `dpkg -l`, `lsmod` attachments

Cross-links: [#12 INTROSPECT](https://github.com/nymble/zpod/issues/12), [#4 M2 audio](https://github.com/nymble/zpod/issues/4), [#6 M4 display/buttons](https://github.com/nymble/zpod/issues/6).

## Vendor / online display evidence (not live-board verified)

**Source class:** Paul pasted UGEEK ZPOD display facts from online/vendor material (2026-10-07 PT). Treat as **candidate** facts until confirmed from assembly guide, Pirate Audio reference matched to ZPOD, or live GPIO / `/dev` readout on a booted board.

| Fact | Value | Status |
| --- | --- | --- |
| Panel | 1.3-inch IPS color LCD | Online only |
| Resolution | 240×240 | Online only |
| Driver chip | ST7789 | Online only |
| Software path (apps) | Pirate Audio / Pimoroni-style Python ST7789 over SPI | Online only |
| Gaming path | Often fb mirroring (`fbcp-ili9341` / `st7789v` → `/dev/fb1`) | Online only |
| Our OS path | Raspberry Pi OS lite via pi-gen (not RetroPie/Volumio as base); primary UI = music/portable apps + buttons; emulation secondary | Project policy |

**Do not invent** BCM pin numbers for DC / BL / CS (or any other TFT GPIO) until confirmed from assembly guide, Pirate Audio pinout matched to this ZPOD, or live `gpio` / device-tree readout.

**Conflict with earlier tree notes:** `config/zpod.yaml` still records vendor-tree names (`pitft22`, 2.2", 320×240, driver unknown) from howardqiao sources. Those remain unmerged with this ST7789 / 1.3" / 240×240 online claim until hardware introspect closes the gap. Prefer live-board evidence over either source if they disagree.

**Next:** once pins are confirmed, software bring-up can start (Python ST7789 SPI and/or fb path); track under [#6 M4](https://github.com/nymble/zpod/issues/6) and [#12 INTROSPECT](https://github.com/nymble/zpod/issues/12).

## Vendor / upstream developer ([howardqiao](https://github.com/howardqiao))

Paul pointed at https://github.com/howardqiao as the developer home for ZPOD-related work. Reviewed public repos via `gh` API + clone (2026-10-07 PT). **Do not invent pinouts** — BCM numbers below are quoted only where a howardqiao file states them. TFT DC/BL/CS pins are **not** stated in `howardqiao/zpod` or `aoide-dac-drivers` ZPOD setup scripts.

### Repos ranked for our RAPID path (display + buttons + audio)

| Rank | Repo | URL | Last push (UTC) | License | Usefulness |
| --- | --- | --- | --- | --- | --- |
| 1 | `aoide-dac-drivers` | https://github.com/howardqiao/aoide-dac-drivers | 2021-03-28 | none declared | Primary: `aoide-zpod-dac` overlays/modules, `aoide_zpod_setup.sh`, TFT/button install scripts |
| 2 | `zpod` | https://github.com/howardqiao/zpod | 2020-09-17 | none declared | Stock player binaries + `zpod_res/` (`config.txt`, `retrogame.cfg`, `aoide-zpod-dac.dtbo`/`.ko`, `fbcp`, `retrogame`) |
| 3 | `AOIDE_KAZOO` | https://github.com/howardqiao/AOIDE_KAZOO | 2020-04-28 | GPL-2.0 | Separate AOIDE portable product: **ST7789** + Pirate-Audio-style Python path; **not** the ZPOD `pitft22` tree — do not copy pins onto ZPOD without live verify |
| 4 | `fbcp-ili9341` | https://github.com/howardqiao/fbcp-ili9341 | 2019-10-17 | MIT (fork of juj/fbcp-ili9341) | Generic SPI LCD mirror; ST7789 supported upstream; no ZPOD-specific pin map in this fork |
| 5 | `ugeek-screen-setup` | https://github.com/howardqiao/ugeek-screen-setup | 2020-03-03 | none declared | Generic uGeek TFT menus (`pitft22` / 2.4" / HD-TFT); pulls `zpod` `fbcp`; not ZPOD-button specific |
| — | `getcoverart` | https://github.com/howardqiao/getcoverart | 2020-04-12 | none declared | Cover-art helper used by player; no hardware map |
| — | `gameducky` | https://github.com/howardqiao/gameducky | 2019-01-01 | none declared | Other uGeek handheld; `pitft22` + JoyBonnet — not ZPOD |
| — | `smartupsv3` / `ugeek-ups2-setup` | (UPS) | 2021 / 2019 | none | Battery/UPS tooling; not display/audio bring-up |

Other howardqiao originals (`RaspiVoiceHAT`, `raspihdtftplus`, `myvolumio`, …) are adjacent AOIDE/uGeek products, not the ZPOD portable stack.

### Install / setup script names (quoted)

From **aoide-dac-drivers**:

- `dac_install.sh` — interactive DAC installer; overlays include `aoide-zpod-dac`
- `aoide_zpod_setup.sh` — full ZPOD RetroPie/player setup (input, screen, sound, AP, samba)
- `aoidetft_setup.sh` — “Aoide PITFT Player” on Raspbian (clones `zpod` + `retrogame`)
- `aoiderp_setup.sh` — same family for RetroPie
- `volumio_aoide_builder.sh` — Volumio builder helper

From **AOIDE_KAZOO** (different product): `install.sh` (Mopidy + ST7789), `setup.sh` (player + `fbcp-ili9341` + retrogame).

From **ugeek-screen-setup**: `screen_setup.sh`.

### Display (what vendor trees actually say)

**ZPOD tree (`zpod` + `aoide_zpod_setup` / `aoidetft` / `aoiderp`):**

- Overlay name: `pitft22` (Adafruit-style overlay **name** only; panel chip **not** named in these files)
- Resolution forced via HDMI CVT: `hdmi_cvt=320 240 60 1 0 0 0` (320×240)
- Example lines from `zpod/zpod_res/config.txt`: `dtoverlay=pitft22,rotate=90,speed=64000000,fps=30` and `dtparam=spi=off`
- Example from `aoide_zpod_setup.sh` `enable_screen`: `dtparam=spi=on` and `dtoverlay=pitft22,speed=80000000,rotate=90,fps=60`
- Mirror path: `fbcp` / `raspi2fb` binaries shipped under `zpod_res/`; started from `rc.local`
- **No ST7789 string** and **no DC/BL/CS BCM numbers** appear in `howardqiao/zpod` or the ZPOD setup scripts under `aoide-dac-drivers`

**AOIDE_KAZOO (quote only — not confirmed as ZPOD hardware):**

- `install.sh` sets `ROTATE=180`, `CS=0`, `DC=25`, `BL=27` and configures Mopidy `[pidi] display = st7789` with packages `pidi-display-st7789`
- `setup.sh` uses `hdmi_cvt=240 240 60 1 0 0 0` and `fbcp-ili9341`
- Audio overlay in KAZOO scripts: `hifiberry-dacplus` (not `aoide-zpod-dac`)

Conflict with Paul-pasted online “ST7789 / 1.3\" / 240×240 / Pirate Audio” claim remains open until live introspect; vendor ZPOD scripts still say `pitft22` + 320×240.

### Buttons (vendor maps disagree — superseded by the live map above; map 1 matches)

All maps below are **as written in howardqiao files** (BCM / Broadcom numbering per `retrogame.cfg` comments). They conflict; leave `config/zpod.yaml` buttons unset until live verify.

1. **`aoide_zpod_setup.sh` → `/boot/retrogame.cfg`:**
   - LEFT 17, RIGHT 23, UP 4, DOWN 22, U 12, I 13, J 16, K 26, EQUAL 5, MINUS 20, RIGHTSHIFT 24, ENTER 6
2. **`aoidetft_setup.sh` / `aoiderp_setup.sh` → `/boot/retrogame.cfg`:**
   - RIGHTSHIFT 5, LEFT 24, RIGHT 22, K 23
3. **`zpod/zpod_res/retrogame.cfg`:**
   - K 5, LEFT 22, RIGHTSHIFT 23, RIGHT 24
4. **`AOIDE_KAZOO/setup.sh` → `/boot/retrogame.cfg`:** UP 5, DOWN 16, LEFT 20, RIGHT 6
5. **`AOIDE_KAZOO/install.sh` Mopidy `[raspberry-gpio]`:** bcm5=volume_up, bcm6=next, bcm16=volume_down, bcm20=play_pause

Also starts `/usr/local/bin/retrogame` and `/home/pi/zpod/volcontrol` from `rc.local` in ZPOD setup.

### Audio / `aoide-zpod-dac` (confirmed in vendor trees)

- Overlay name: `aoide-zpod-dac` (in `zpod_res/config.txt`, `aoide_zpod_setup.sh` `enable_sound`, and driver tarballs under `aoide-dac-drivers/drivers/`)
- Bundled artifacts: `zpod/zpod_res/aoide-zpod-dac.dtbo`, `aoide-zpod-dac.ko`
- DTBO strings: compatible `aoide,aoide-zpod-dac`; codec node `pcm5122@4d` / `ti,pcm5122` on i2c1; binds I2S controller — **no GPIO pin map for the DAC in the dtbo**
- Driver tarball e.g. `drivers/aoide_dac_5.10.17.tar.gz` ships matching `.dtbo` + `.ko` for several kernel flavours
- **Installer menu bug (quote):** in `dac_install.sh`, menu label `"5" "AOIDE ZPOD DAC"` sets `dtoverlay=aoide-zero-digiplus`, while `"6" "Raspi Voice HAT"` sets `dtoverlay=aoide-zpod-dac`. Prefer invoking `sudo ./dac_install.sh aoide-zpod-dac` (script accepts overlay name as `$1`) or editing config by hand — do not trust the numbered menu alone
- IR in stock `zpod_res/config.txt`: `dtoverlay=gpio-ir,gpio_pin=7`

### License notes

- Most ZPOD-critical repos (`zpod`, `aoide-dac-drivers`, `ugeek-screen-setup`, `getcoverart`) declare **no license** on GitHub
- `AOIDE_KAZOO`: GPL-2.0
- `fbcp-ili9341` fork: MIT (upstream juj)
- Treat unlicensed vendor binaries/scripts as reference only; prefer reimplementation / documented overlays under our tree for shipping

### What this does **not** confirm

- Any BCM map for TFT **DC / backlight / chip-select** on ZPOD
- That the live UGEEK unit is ST7789 240×240 vs vendor-script `pitft22` 320×240
- A single authoritative button map
