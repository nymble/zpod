# ZPOD buttons → keyboard events (`zpod-buttons`)

`zpod-buttons.service` (`/usr/lib/zpod/zpod_buttons.py`) polls the button
GPIOs every 5 ms through `/dev/gpiomem`, debounces them (20 ms), and publishes
a uinput keyboard named **`zpod-buttons`**. Any app can read it
(`evtest`, SDL, pygame, python-evdev, a terminal on tty1, …).

- GPIOs are **BCM**, active-low, internal pull-ups (set in `config.txt` for
  firmware time and again by the daemon at start).
- No GPIO line is claimed and no pin function is changed. **GPIO20 stays the
  I2S DIN pin**; the daemon only sets its pull-up (GPPUD sequence on BCM2835)
  and reads the level register.
- **D-pad centre** has no pin. It is reported as `KEY_ENTER` when GPIO 4, 17,
  22 and 23 all go low within 50 ms (observed ~40 ms). Arrow keys are emitted
  only after that window, so a centre press never leaks an arrow.
- Held keys auto-repeat (kernel `EV_REP`, 400 ms delay, 120 ms period).

## Default map

| Button | GPIO | Key code | Notes |
| --- | --- | --- | --- |
| D-pad up | 4 | `KEY_UP` | |
| D-pad down | 22 | `KEY_DOWN` | |
| D-pad left | 17 | `KEY_LEFT` | |
| D-pad right | 23 | `KEY_RIGHT` | |
| D-pad centre | 4+17+22+23 | `KEY_ENTER` | combo, see above |
| Home | 24 | `KEY_HOME` | |
| Play/Pause | 6 | `KEY_PLAYPAUSE` | |
| Triangle | 13 | `KEY_I` | letters match the vendor `retrogame.cfg` (U/I/J/K) so old emulator keymaps carry over |
| Square | 12 | `KEY_U` | |
| Circle | 26 | `KEY_K` | |
| X | 16 | `KEY_J` | |
| Side volume up | 5 | `KEY_VOLUMEUP` | **also** raises ALSA `Digital` on the DAC by 3 % (repeats after 0.6 s held, then every 0.15 s) |
| Side volume down | 20 | `KEY_VOLUMEDOWN` | **also** lowers ALSA `Digital` by 3 % (same repeat); GPIO20 = I2S DIN |

Because `zpod-buttons` already changes the DAC volume, apps should treat
`KEY_VOLUMEUP/DOWN` as informational (show an OSD) and not apply them again.
Set `volume_adjusts_alsa = false` in `/etc/zpod/buttons.conf` if an app wants
to own the volume keys.

## Vendor labels (uGeek ZPOD user guide, 2017)

From [`docs/vendor/ZPOD-HiFi-Music-Player-User-Guide-2017.pdf`](./vendor/README.md),
page 1, "The functions of the buttons":

| Button | Vendor function |
| --- | --- |
| Up | put one song on repeat |
| Down | shuffle |
| Left / Right | left / right |
| Home | home (back to menu) |
| Pause | pause / play |
| Triangle | clock on/off |
| Circle | OK / playing queue |
| Square | back to menu |
| X | cancel |
| + / − (side) | volume up / down |

## How `zpod-ui` uses them (M3)

`zpod-ui` follows the vendor labels where they fit a status/menu screen. The
player-only meanings (Up = repeat, Down = shuffle, Circle = playing queue) are
reserved for the music player (M4+); in menus Up/Down move the selection.

| Key | Button | Action |
| --- | --- | --- |
| UP / DOWN | D-pad up / down | move menu selection (vendor: repeat / shuffle, player only) |
| ENTER, `KEY_K`, RIGHT | centre, Circle, right | **OK** / select; also "yes" in Reboot / Shutdown confirmation |
| `KEY_J`, LEFT | X, left | **cancel** / back; "no" in confirmations |
| HOME, `KEY_U` | Home, Square | **back to menu** (main screen) |
| `KEY_I` | Triangle | **clock on/off** (full-screen clock; press again or Home to leave) |
| PLAYPAUSE | Play/Pause | start / stop the 440 Hz test tone (until there is a player) |
| VOLUMEUP / VOLUMEDOWN | side + / − | volume overlay (ALSA already changed by `zpod-buttons`) |

Menu: **Status, Play test tone, Volume, Wi-Fi info, Reboot, Shutdown**
(Reboot and Shutdown ask "OK (Circle / centre) = yes / X / Left = cancel").

Confirmed on hardware 2026-10-17 (v0.3.0-m2, `evtest`): every button emits
exactly one press/release of the code above; centre gives only `KEY_ENTER`
(no arrow leak); Circle = `KEY_K` (GPIO26), X = `KEY_J` (GPIO16); side volume
changes `Digital` (166 → 190 → 196). An ordinary ~0.6 s press used to trigger
volume repeats (0.4 s delay) — M3 raises the delay to 0.6 s.

## Overrides

`/etc/zpod/buttons.conf` (INI) can remap key codes and tune
`combo_window_ms`, `debounce_ms`, `volume_step_pct`, `volume_adjusts_alsa`.
Restart with `sudo systemctl restart zpod-buttons`.

## Notes

- The keyboard is a normal input device, so the active console (tty1) also
  receives the keys. That is harmless at a login prompt; it is how apps
  without evdev support can use the buttons.
- Debug: `sudo journalctl -u zpod-buttons`, `sudo evtest` (pick
  `zpod-buttons`), or read raw levels with `pinctrl get 4,5,6,12,13,16,17,20,22,23,24,26`.
