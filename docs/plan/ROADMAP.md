# ZPOD roadmap (small pieces)

Parent plan: [../../PLAN.md](../../PLAN.md) and [#1](https://github.com/nymble/zpod/issues/1).
Button/menu spec: [../ui/BUTTONS-AND-MENUS.md](../ui/BUTTONS-AND-MENUS.md).
Status: **done** (on hardware), **built** (in an image or tested off-device,
not yet on hardware), **next**, **future** (not committed).

## Hardware test, 2026-10-22 (Paul, `v0.4.1-m3` on the Pi Zero W Rev 1.1)

| Area | Result |
| --- | --- |
| Boot | **pass**: menu after about 1 min 32 s (M2: 3 min 11 s) |
| Wi-Fi via `zpod-wifi.txt` on the boot volume | **pass**: joined, file consumed, no credentials in the image |
| Battery (MAX17048) | **pass**: % and voltage shown |
| Shutdown screen | pass (as on 2026-10-17) |
| Volume (side buttons) | **fail**: overlay disappears while the button is held; the value jumps instead of stepping |
| USB networking to the Mac | **fail**: no DHCP lease; at diagnosis time the Mac's USB tree showed no ZPOD device at all (cable/port or no enumeration). Root cause of the DHCP failure: macOS AppleUserECM bug, see [../USB-GADGET.md](../USB-GADGET.md) |
| Wi-Fi info screen | **fail (missing)**: no MAC address |
| Buttons/menus | **change requested**: Triangle should not be a clock key, Play should be play/pause (not the tone), no Volume menu item, Left = back everywhere, multi-level menus |

## Action items from this round

| # | Item | Status | Where |
| --- | --- | --- | --- |
| A1 | Volume: one writer (zpod-buttons), integer steps of 2 %, cap 80 %, 75 ms min press, repeat from 650 ms every 150 ms, `/run/zpod/volume`, overlay stays while held | **built** (sim test) | `zpod_buttons.py`, `zpod-core/volume.rs`, platform overlay |
| A2 | USB: configfs CDC-NCM gadget, ECM/g_ether selectable via `zpod-usb.txt`, fixed MACs, DHCP unchanged | **built** | `zpod-usb-gadget.sh` |
| A3 | Wi-Fi info shows the MAC | **built** | launcher + Python UI |
| A4 | Menu tree: Home = top, Left/X = back, Right/OK/Circle = enter, Play global, volume global, Clock is an app, tone in Diagnostics, no Volume item | **built** (7 menu tests + simulator) | `launcher/src/menu.rs` |
| A5 | DS3231 RTC: `dtoverlay=i2c-rtc,ds3231`, `zpod-rtc` (`hwclock -s` at boot, `-w` when NTP-synced and at shutdown), no fake-hwclock, set time in Settings | **built** | stage2-05, `zpod-rtc.sh` |
| A6 | GPIO27 backlight test (inferred) | **built** | Settings > Display |
| A7 | SPI 48/64 MHz trial with 10 s auto-revert; default 32 MHz | **built** | Settings > Display |
| A8 | Low-battery power-off < 5 % (with voltage + not-charging guard); 6-step battery icon | **built** | `zpod_battery_guard.py`, `status.rs` |
| A9 | Wire the M4 Rust launcher into the image build (uid 1000, not `pi`) | **built** | `build/tweaks/apply.sh` |
| A10 | Paul: hardware pass of the M4 candidate ([../TEST-M4.md](../TEST-M4.md) section 9) | **next** | |
| A11 | Bluetooth join test (M3 scope) | **next** | #5 |

## Milestones

### M4: launcher, apps, fixes (in progress)

1. M4.1 Rust launcher + menu tree + shared volume overlay: built.
2. M4.2 Apps: Music (artists/albums/playlists, album art), Clock (3 faces, seconds), Tetris: built; first hardware run pending.
3. M4.3 File & app transfer over USB and Wi-Fi (scp/rsync, `zpod-drop`, `zpod-app`): built.
4. M4.4 Boot splash: built.
5. M4.5 USB NCM gadget: built; **must pass on Paul's Mac** before release.
6. M4.6 RTC, low battery, SPI speed option, backlight test: built.
7. M4.7 Image `v0.5.0-m4` release: after A10 passes (not published yet).

### Next (after M4)

1. Wi-Fi scanner app (list networks; may need an external USB adapter for monitor mode).
2. Brightness (PWM on GPIO27) once the backlight test proves GPIO27.
3. Music: playlist editing, album-art browser.
4. Buildroot image (smaller, faster boot) with the same apps.
5. Bluetooth audio / pairing (M3 leftovers).

### Future (not committed)

1. IR receiver validation: there may be one behind the front hole above Home; Howard's config has `dtoverlay=gpio-ir,gpio_pin=7` (unverified; GPIO7 = SPI0 CE1).
2. Game emulation (RetroArch); Howard's U/I/J/K key map already matches ours.
3. Clock alarm; AirPlay receiver; BitChat (M8).

## Pin facts used by these items

| Fact | Status |
| --- | --- |
| Buttons BCM 4/22/17/23 (D-pad), 24 Home, 6 Play, 13 Triangle, 12 Square, 26 Circle, 16 X, 5 Vol+, 20 Vol− | confirmed |
| PCM5122 DAC @ 0x4d, MAX17048 @ 0x36, DS3231 @ 0x68 on I2C1 | confirmed (i2cdetect) |
| ILI9340 320×240 on SPI0 CE0, DC = GPIO25 | confirmed |
| GPIO27 = backlight | **inferred** (held high; vendor KAZOO `BL=27`) |
| SPI clock above 32 MHz is stable | **unknown** (test in Settings > Display) |
| IR receiver on GPIO7 | **unverified** |
