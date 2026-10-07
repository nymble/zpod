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
| Side volume up | 5 | `KEY_VOLUMEUP` | **also** raises ALSA `Digital` on the DAC by 3 % (repeats while held) |
| Side volume down | 20 | `KEY_VOLUMEDOWN` | **also** lowers ALSA `Digital` by 3 %; GPIO20 = I2S DIN |

Because `zpod-buttons` already changes the DAC volume, apps should treat
`KEY_VOLUMEUP/DOWN` as informational (show an OSD) and not apply them again.
Set `volume_adjusts_alsa = false` in `/etc/zpod/buttons.conf` if an app wants
to own the volume keys.

## How `zpod-ui` uses them

| Key | Action |
| --- | --- |
| UP / DOWN | move menu selection |
| ENTER (centre), RIGHT, Circle | select |
| LEFT, X | back / cancel |
| HOME | back to the main screen |
| VOLUMEUP / VOLUMEDOWN | volume overlay |

Menu: **Status, Play test tone, Volume, Wi-Fi info, Reboot, Shutdown**
(Reboot and Shutdown ask "ENTER = yes / LEFT or X = no").

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
