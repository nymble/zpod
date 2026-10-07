# M2 on-device test checklist — display + audio + buttons (`v0.3.0-m2`)

Board: Pi Zero W in the ZPOD case. Image: `2026-10-07-zpod-pi0w-m2.img.xz`
(verify against `SHA256SUMS` first). Never flash a card you care about.

## 0. Before first boot (on the Mac)

1. Write the image (Raspberry Pi Imager → *Use custom*, or `xz -d` + `dd`).
2. Re-mount the card; on the `bootfs` volume create the user headlessly
   (the data USB port is a gadget now, so an OTG keyboard will not work for
   the HDMI wizard — see [USB-GADGET.md](./USB-GADGET.md)):

   ```bash
   echo "paul:$(openssl passwd -6)" > /Volumes/bootfs/userconf.txt
   ```
3. Home SSID is configured in the image. Eject, insert, power on.

## 1. Boot + screen

| # | Check | Pass |
| --- | --- | --- |
| 1.1 | LCD within ~60–90 s of power | `zpod-ui` main screen: **ZPOD** header, battery % and HH:MM top-right, menu (Status, Play test tone, Volume, Wi-Fi info, Reboot, Shutdown), `wlan0` / `usb0` IPs at the bottom |
| 1.2 | Orientation | Text upright in landscape, not mirrored, fills 320×240 |
| 1.3 | Colours | Header text yellow, selection bar blue (not orange → would mean RGB/BGR swap) |
| 1.4 | Battery | % shown in the header and roughly plausible vs charge state (MAX17048 @ 0x36) |

## 2. Buttons

| # | Check | Pass |
| --- | --- | --- |
| 2.1 | D-pad up/down | selection moves, wraps around |
| 2.2 | D-pad centre | opens the selected item (no extra up/down move first) |
| 2.3 | Left / X / Home | back to main screen |
| 2.4 | Status | time, IPs, battery V, volume %, DAC ok, uptime |
| 2.5 | Side volume up/down | "Vol NN%" overlay; value changes in 3 % steps; hold repeats |
| 2.6 | All keys | `sudo evtest` → select `zpod-buttons`; each button prints the code in [BUTTONS.md](./BUTTONS.md) (centre = `KEY_ENTER`, Triangle `KEY_I`, Square `KEY_U`, Circle `KEY_K`, X `KEY_J`, Home `KEY_HOME`, Play `KEY_PLAYPAUSE`) |

## 3. Audio

| # | Check | Pass |
| --- | --- | --- |
| 3.1 | Menu → Play test tone | 440 Hz in headphones for 2 s, then "Tone done" |
| 3.2 | `aplay -l` | `sndrpiaoidezpod` card present |
| 3.3 | `speaker-test -c 2 -t sine -f 440 -l 1` (no `-D`) | plays on the DAC (default device) |
| 3.4 | Mixer defaults | `amixer -c sndrpiaoidezpod sget Digital` ≈ 80 %, `sget 'Analogue Playback Boost'` = on |
| 3.5 | Module provenance | `dkms status` → `aoide-zpod-dac/1.0.0, 6.18.50+rpt-rpi-v6, armv7l: installed` (arch label comes from the qemu build chroot; harmless); `modinfo aoide-zpod-dac` filename under `updates/dkms`, `lsmod \| grep aoide` |
| 3.6 | Amp/console | `cat /proc/cmdline` has no `console=serial0`; `pinctrl get 14` → `op dl` |
| 3.7 | Volume persists | change volume, reboot, same value |

## 4. USB cable link (Mac)

| # | Check | Pass |
| --- | --- | --- |
| 4.1 | Data cable Mac ↔ Pi `USB` port | Mac shows new Ethernet interface with `10.12.194.x` |
| 4.2 | `ssh paul@10.12.194.1` | login works with Wi-Fi **off** on the Mac |
| 4.3 | `ssh paul@zpod.local` | resolves and logs in |
| 4.4 | Mac internet | still works via its own Wi-Fi (no default route via the Pi) |
| 4.5 | Screen | `usb0 10.12.194.1` on the main screen |

## 5. Wi-Fi + debug

| # | Check | Pass |
| --- | --- | --- |
| 5.1 | `iw dev wlan0 get power_save` | `Power save: off` (also shown in Wi-Fi info screen) |
| 5.2 | Long session in the case | no drop-outs over ~30 min of `ping -i 5 <router>` |
| 5.3 | Persistent journal | `journalctl --list-boots` shows previous boots after a reboot |
| 5.4 | Tools | `i2cdetect -y 1` → `36`, `UU` at `4d`, `68`; `gpioinfo` works |
| 5.5 | Reboot / Shutdown menu | asks for confirmation; ENTER reboots / powers off; LEFT cancels |

Report results on [#4](https://github.com/nymble/zpod/issues/4) (audio),
[#6](https://github.com/nymble/zpod/issues/6) (display/buttons) and
[#7](https://github.com/nymble/zpod/issues/7) (USB path). Useful logs:
`journalctl -b -u zpod-ui -u zpod-buttons -u zpod-audio-init`, `dmesg | grep -i -E 'aoide|pcm512|dwc2|g_ether'`.

## Known gaps (not verifiable without hardware at build time)

- GPIO27 is assumed to be backlight/power enable (held high), not proven.
- `dwc2,dr_mode=peripheral` + fixed-MAC `g_ether` with macOS is untested on this unit.
- DS3231 RTC is not enabled (time comes from NTP / fake-hwclock).
- Kernel `rpi-v8` (arm64) has no DKMS build of the DAC driver; the Zero W never boots it.
