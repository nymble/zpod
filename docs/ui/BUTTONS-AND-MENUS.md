# ZPOD buttons and menus (M4/M5/M6 launcher)

This is the reference for how the ZPOD buttons behave in the M4 Rust launcher
and its apps. The GPIO-to-key map is in [../BUTTONS.md](../BUTTONS.md).
Code: `apps/crates/zpod-core/src/nav.rs` (shared navigation),
`apps/crates/launcher/src/menu.rs` (menu tree),
`apps/crates/zpod-core/src/volume.rs` (volume overlay),
`overlay/usr/lib/zpod/zpod_buttons.py` (GPIO → keys, volume).

Status labels: **M4** = in the M4 candidate; **M5** = in v0.6.0-m5;
**M6** = in v0.9.0-m6-rc1; **next** = planned for the release after M6; **future** = wanted but not
committed.
Hardware labels: **confirmed** = measured on Paul's ZPOD; **inferred** = from
vendor files or wiring, not yet proven.

## 1. Rules (all screens)

| Button | Key | Role | Scope |
| --- | --- | --- | --- |
| Home | `KEY_HOME` | go to the **top launcher menu** (closes the app) | global |
| Play/Pause | `KEY_PLAYPAUSE` | **play / pause music** (MPD) on the top menu and in Music; on every other launcher screen (Tools, Settings, ...) it **only pauses music that is playing** and otherwise does nothing (test3; before, Play on a tool screen said "Nothing queued") | global |
| Vol + / − (side) | `KEY_VOLUMEUP/DOWN` | change the volume; one overlay for every screen | global |
| Up / Down | `KEY_UP/DOWN` | move the selection; repeat while held | default |
| Right, D-pad centre, Circle | `KEY_RIGHT`, `KEY_ENTER`, `KEY_K` | **enter / OK** | default |
| Left, X | `KEY_LEFT`, `KEY_J` | **back one level** (Left at the top = nothing) | default |
| Triangle, Square | `KEY_I`, `KEY_U` | page-local (scanners: Square = rescan, Triangle = sort; File transfer: Square = rsync share; Saved networks: Square = refresh, Triangle twice = forget; Hotspot password: Triangle = new password) | app/page |

"Global" bindings are handled before an app or menu sees the key and cannot
be overridden, with two documented exceptions: Tetris uses Play to pause
the game, the Button test screen captures every key (Home exits it), and the
alarm/timer ring screen (section 4) uses Home and Play to stop the alarm.
"Default" bindings may be overridden by an app (section 5).

Removed in M4 (Paul's test, 2026-10-22): Triangle no longer opens the clock
(Clock is an app in the menu); Play no longer plays the test tone (moved to
Settings > Diagnostics); the Volume menu item is gone (side buttons only).

## 2. Button state machine

`zpod-buttons` turns GPIO levels into key events. The D-pad centre has no
pin of its own: all four directions low within 50 ms means `KEY_ENTER`.

```mermaid
stateDiagram-v2
    [*] --> Notice: /run/zpod/boot-notice not empty (M6)
    [*] --> Top
    Notice --> Top: OK (file moved to /var/lib/zpod/boot-notice.last) / Home
    Top --> Tools: Tools
    Top --> Settings: Settings
    Settings --> About: About
    Tools --> WiFi: Wi-Fi (status card)
    Tools --> BT: Bluetooth
    Tools --> USB: USB
    Tools --> Battery: Battery
    Tools --> Transfer: File transfer
    WiFi --> Mode: Mode
    Mode --> Job: OK = Off / Client / Hotspot
    WiFi --> Networks: Networks nearby (client)
    Networks --> Detail: OK
    Detail --> Job: OK = join (saved or open)
    WiFi --> Saved: Saved networks (client)
    Saved --> Detail: OK
    Saved --> Job: Triangle twice = forget
    WiFi --> Devices: Connected devices (hotspot)
    WiFi --> Password: Password (hotspot)
    Password --> Job: Triangle = new password
    WiFi --> WTools: Wi-Fi tools
    WTools --> Channels: Channel utilization
    WTools --> Addresses: Devices & addresses
    WTools --> Server: Drop box / Book catalog
    Server --> Job: OK = on/off
    Job --> WiFi: done (toast, card refreshed)
```

## 3. Menu tree

Top level (Home always lands here):

| Item | Status | Notes |
| --- | --- | --- |
| **Music** | M4, M5 | M5: playlists (add the playing track or album, play, shuffle play, delete), hold Up/Down = jump A-Z (Artists, Playlists) or page (long lists), always-visible Repeat/Shuffle controls on Now Playing. M4: Artists / Albums (both go artist → albums by year → tracks; counts in titles and rows, album year shown), playlists, queue, now playing with album art and year; Play/Pause global. See MUSIC-PLAYER.md |
| **Clock** | M4, M5 menu | 3 faces (digital, analog, words). M5: Up/Down = face, Right/OK/Circle = **Clock menu** (Alarm >, Timer >, Face, Seconds, 12/24 hour), Square = seconds, Triangle = 12/24 h. See section 4a |
| **Tetris** | M4 | SRS rotation, hold, ghost, high scores |
| (installed apps) | M4 | anything installed with `zpod-app install` appears here |
| **File transfer** (test3: under Tools) | M4, M5 | M5: USB IP, Wi-Fi IP + `zpod.local`, browser (drop server), rsync share state, free space, library song count + last scan (or "rescanning..."), newest 4 files. **OK = rescan library now**, **Square = rsync share on/off** (USB only, off by default). Refreshes every 3 s (1 s while scanning) |
| **Battery** (test3: under Tools) | M6 | its own page (was "Battery & about"): big %, charging state, time left, last full charge, typical runtime, discharge chart. Section 8 |
| **Tools >** | M5, M6, test3 | test3: **Wi-Fi, Bluetooth, USB, Battery, File transfer** (in that order) |
| **Settings >** | M4 | |
| **Power >** | M4 | |

Settings:

| Item | Status | Notes |
| --- | --- | --- |
| Display > Backlight test | M4 | GPIO27 low for 3 s, then high. **GPIO27 = backlight is inferred** (vendor KAZOO `BL=27`, held high by the vendor image) |
| Display > Brightness | next | PWM on GPIO27, only if the backlight test proves it switches the LED |
| Display > Panel SPI speed | M4 | cycles 32 → 48 → 64 MHz; the new speed is kept only if you press OK within 10 s, otherwise it reverts (default stays 32 MHz) |
| Date & time | M4 | set date/time; OK saves with `date -s` + `hwclock -w` (DS3231) |
| About | M4 (M4.1: build, SD card, login; M6: battery moved out) | **version** (from `/etc/zpod-release`: version, git commit, build date), **SD card** size and use per partition (root, data, boot), host + login user (or "NOT SET UP"), RTC time, kernel, core version |
| Installed apps | M4 | name, id, version |
| Diagnostics > System status | M4 (M4.1: SD, build) | time, IPs, battery, volume, DAC, music service, uptime, **SD card** summary, **build** + core version |
| Diagnostics > Storage & build | M4.1, M5 | image version, git commit, build date, image name; SD card size; use per partition: **Root /**, **Data** (`/data`, M5 large-card layout; hidden on older images), **Music** (`/srv/zpod/media`; "on data" with the M5 layout, "on root" on older images), **Boot** (`/boot/firmware`), each as used / total, %, free (red at 90 %+); login user, or "NOT SET UP" if the card was booted without `userconf.txt` / `zpod-ssh.pub` |
| Diagnostics > Button test | M4 | shows each key as it is pressed, repeats, last event; Home exits |
| Diagnostics > Tone test (440 Hz) | M4 | start/stop the sine tone (Play also stops it) |

Tools (test3 layout, Paul's spec 2026-11-01; M5/M6 screens regrouped):

| Item | Status | Notes |
| --- | --- | --- |
| **Wi-Fi** | test3 | One screen. **Status card**: mode pill (Off grey / Client cyan / Hotspot amber), SSID, IP, channel, signal (dBm + words, link rate), MAC; refreshed every 5 s on a background thread. Rows by mode — **Off**: Mode, Wi-Fi tools. **Client**: Mode, Networks nearby, Saved networks, Wi-Fi tools. **Hotspot**: Mode, Connected devices (count), Password, Wi-Fi tools |
| Wi-Fi > Mode | test3 | **Off / Client / Hotspot**, current one marked. Off = `nmcli radio wifi off` (no wpa_supplicant/hostapd left); Client = radio on, hotspot down, rejoin saved networks; Hotspot = radio on + hotspot (made on first use, least busy of 1/6/11). Leaving Hotspot goes to whatever you pick |
| Wi-Fi > Networks nearby | M5 scanner, test3 | the scanner list (Square = rescan, Triangle = sort). **OK = network details** with the **live signal meter** (the standalone Signal meter is gone). **OK on the details = join**: a saved network connects; an **open** network is joined and saved (`nmcli device wifi connect`); a network with a password opens **password entry** (test6: USB keyboard if plugged in, otherwise the button character grid; [TEXT-ENTRY.md](./TEXT-ENTRY.md)) |
| Wi-Fi > Saved networks | M6, test3 | profiles; **OK = details + meter (OK there = connect)**, **Triangle twice = forget** (`nmcli connection delete`), Square = refresh |
| Wi-Fi > Connected devices | test3 | hotspot clients: name (DHCP host name) or IP, MAC, connected time, signal (`iw dev wlan0 station dump` + DHCP leases + ARP). Refreshes every 5 s |
| Wi-Fi > Password | M6 hotspot, test3 | SSID, password hidden until **OK = show/hide**, **Triangle = new password** (clients must re-join), address and drop box URL. Stored only in the root-only hotspot profile (`/etc/NetworkManager/system-connections/zpod-hotspot.nmconnection`; on Buildroot that is `/data/etc/NetworkManager/...`), never in the repo or image |
| Wi-Fi > Wi-Fi tools > Channel utilization | test3 | 2.4 GHz axis ch 1-13; **one curve per network** spanning its ~22 MHz (about ±2 channels), height = RSSI (−95 to −30 dBm); columns shaded by overlap-weighted load; **green marker on the least busy of 1/6/11**; yours in cyan; SSIDs labelled for the strongest six. OK = rescan |
| Wi-Fi > Wi-Fi tools > Devices & addresses | test3 | passive: LAN neighbours from the ARP table (IP, MAC) and every access point heard (BSSID, SSID, channel, security, dBm). OK = rescan. Nothing is probed or sent |
| Wi-Fi > Wi-Fi tools > Drop box, Book catalog | M6 | as before (moved from Tools) |
| Bluetooth | M5, test3 | the Bluetooth scanner. test3: an empty `btmgmt find` counts as a failure (falls back to `bluetoothctl`), the `bluetoothctl` parser handles coloured `[CHG] Device … RSSI` lines, and `bluetoothctl devices` is merged in (names) |
| USB | test3 | landing page: gadget function (CDC-NCM), controller state + speed (`/sys/class/udc`), usb0 up/down + 10.12.194.1, the host's IP/MAC (ARP) and DHCP host name, traffic. Read-only; Bash Bunny-style tools later |
| Battery | M6 | unchanged, moved from the top menu |
| File transfer | M4, M5 | unchanged, moved from the top menu |

Status bar Wi-Fi indicator (test3): **Off** = grey bars with a red strike;
**Client** = 0-4 bars from the link signal (an amber dot while not
connected); **Hotspot** = a cyan antenna glyph plus the number of connected
devices.

Why the scanners live in the launcher: triggering a Wi-Fi scan and Bluetooth
discovery needs CAP_NET_ADMIN. The launcher already runs as root (it owns the
LCD, power and the clock), so it is the privileged helper; apps run as the
unprivileged `zpod` user and cannot scan. No setuid binary or file
capability is added.

Moved in M5: **Wi-Fi info** from the top menu to Tools (test3: merged into the Wi-Fi status card). Diagnostics (System
status, Storage & build, Button test, Tone test) stays under Settings: they
check this ZPOD's own hardware, Tools look at the world around it.

Power: **Reboot**, **Shut down** (both ask: OK/Circle = yes, Left/X = no).

Other planned items (not in the menu yet):

| Item | Status | Notes |
| --- | --- | --- |
| Boot splash | M4 | `zpod-bootsplash` from early boot until the launcher starts |
| App transfer over USB + Wi-Fi | M4 | `scripts/zpod-app-push.sh`; menu page explains it |
| Music: album art browser, reorder/rename playlists | next | art is shown in Now Playing; M5 adds create/add/play/delete |
| Clock: alarm + timer | M5 | in the Clock menu; rung by the launcher (section 4a) |
| IR receiver validation | future | there may be an IR receiver behind the front hole above Home. Howard's `config.txt` has `dtoverlay=gpio-ir,gpio_pin=7` (**unverified**; GPIO7 is SPI0 CE1, unused by our LCD on CE0) |
| Game emulation (RetroArch) | future | Howard's RetroArch map (a=K Circle, b=J X, x=I Triangle, y=U Square, hotkey=Home) matches our key codes |
| Buildroot image (fast boot) | next (started in parallel with M5, branch `buildroot`) | smaller, faster-booting image with the same apps |
| Bash Bunny-style USB gadget app | future | docs/research/BASH-BUNNY.md |

## 4. Per-screen actions

| Screen | Up/Down | Right / OK / Circle | Left / X | Other |
| --- | --- | --- | --- | --- |
| Menu (any level) | move (wraps), repeat | enter item / open app | back one level (nothing at top) | — |
| Wi-Fi (test3) | move | open row | back | card refreshes every 5 s |
| Wi-Fi mode (test3) | move | switch | back | — |
| File transfer (M5) | — | OK = rescan music library | back | Square = rsync share on/off |
| Wi-Fi / Bluetooth scanner (M5) | move, repeat | OK/Right/Circle = details | back | Square = rescan, Triangle = sort |
| Bluetooth details (M5) | — | — | back | — |
| Wi-Fi network details + meter (test3) | — | OK = join (saved / open) | back | updates 1/s (connected) or per scan |
| Battery (M6) | — | — | back | refreshes every 20 s |
| Channel utilization (test3) | — | OK = rescan | back | Square = rescan |
| Devices & addresses (test3) | move | OK = rescan | back | — |
| Saved networks (M6, test3) | move | OK = details | back | Square = refresh, Triangle twice = forget |
| Connected devices (test3) | move | OK = refresh | back | every 5 s |
| Wi-Fi password entry (test6) | move in the grid, repeat | OK/Circle = type / run cell (Done = join) | X = delete, cancel when empty | Square = next set, Triangle = show/hide; USB keyboard: Enter/Esc/Backspace/Tab |
| Hotspot password (M6, test3) | — | OK = show/hide | back | Triangle = new password (clients must reconnect) |
| USB (test3) | — | OK = refresh | back | every 3 s |
| Drop box, Book catalog (M6) | — | OK = on/off | back | — |
| Boot notice (M6) | — | OK = got it (dismiss) | — | Home = dismiss, menu |
| Alarm / timer ringing (M5, over any screen or app) | alarm: snooze | alarm: OK/Circle/Right = **stop** | alarm: snooze | alarm: Home/Play = stop, Triangle/Square = snooze; **timer: any key stops**; Vol keys = volume (never stop/snooze) |
| Installed apps, About, System status, Storage & build | — | — | back | — |
| Date & time editor | change field value (repeat) | Right = next field; OK/Circle = **save** | Left = previous field (on the first field = back, no save); X = cancel | — |
| Confirm (Reboot/Shut down) | — | OK/Circle = yes | no | — |
| SPI speed trial | — | OK = keep new speed | back (reverts in 10 s anyway) | — |
| Button test | captured | captured | captured | Home exits |
| Music lists | move; **hold = fast**: after 6 repeats jump to the next/previous letter (Artists, Playlists; a big letter shows) or a page (other lists) | open / play | back; at the Music top = exit | Square = add to queue, Triangle = now playing |
| Music playlists (M5) | move, hold = A-Z | open playlist menu: Play, Shuffle play, Tracks, Delete playlist | back | Square = add the playlist to the queue |
| Music "Add to playlist" (M5) | move | add to the selected playlist (first row "New playlist" makes "Playlist N") | cancel | — |
| Music delete playlist (M5) | — | OK/Circle/Right = delete (music files stay) | cancel | — |
| Music now playing | Up = repeat mode (off/all/one), Down = shuffle; both shown as pills at the bottom | OK/Circle = queue; **Right = next** (hold = seek +10 s) | **Left = previous**; X = back | M5: **Square = add track to a playlist**, **Triangle = add album to a playlist** |
| Clock face (M5) | Up/Down = face | Right/OK/Circle = Clock menu | exit | Square = seconds, Triangle = 12/24 h |
| Clock menu (M5) | move | Alarm > / Timer > / toggle Face, Seconds, 12/24 hour | back to face | — |
| Clock alarm (M5) | move | Alarm on/off, Time > (editor), Repeat (Daily → Weekdays → Weekends → Once), Snooze (5/9/10/15 min), Test: ring in 5 s | back | — |
| Clock alarm time / timer editor (M5) | change hours/minutes (timer: minutes / seconds in 5 s steps) | Right = next field; OK/Circle = save (alarm, switches it on) / start (timer) | Left = previous field, on the first field = back; X = cancel | — |
| Clock timer running (M5) | — | OK/Circle = stop timer | back (keeps running) | — |
| Tetris | Up = hard drop, Down = soft drop | Right = move, Circle/OK = rotate | Left = move, X/Triangle = rotate ccw | Square = hold, **Play = pause** |

## 4a. Alarm and timer (M5)

- Set in **Clock > Alarm** and **Clock > Timer**; saved in
  `$ZPOD_DATA_DIR/clock/alarm.conf` (`/data/zpod/appdata/clock/alarm.conf`),
  so they survive a reboot. The status bar title shows `Alarm 07:00` or
  `Timer 04:59` on the clock face.
- The **launcher** rings them, so they work with the Clock app closed and
  over any other app (it closes the app first, like Home). Time comes from
  the system clock, which `zpod-rtc` sets from the DS3231 at boot.
- Ringing: MPD is paused and its output released (the DAC is exclusive), a
  beep loops through `aplay` at **at least 60 %** volume (your volume is
  restored afterwards), and stops by itself after 5 minutes. Music stays
  paused afterwards.
- Alarm: **OK/Circle/Right/Home/Play = stop**; Up/Down/Left/X/Triangle/Square
  = **snooze** (5/9/10/15 min, set in the Alarm screen). "Once" alarms switch
  off after they ring. A missed alarm (ZPOD off at that time) rings if the
  ZPOD is on within 2 minutes of the alarm time, otherwise it is skipped.
- Timer: any button stops it.
- **No wake from power-off.** The power switch cuts power to the Pi and the
  DS3231 alarm pin is not known to be wired to anything that can switch it
  back on (a halted Pi only wakes on GPIO3 low). The alarm only rings while
  the ZPOD is running; leave it on (screen dims are not implemented yet).

## 5. App conventions

1. Never apply `KEY_VOLUMEUP/DOWN`: `zpod-buttons` already set the volume and
   the platform layer draws the overlay. Do not draw your own.
2. Home exits the app (the launcher shows the top menu). Do not catch it,
   except a capture screen like Button test, which must say "Home exits".
3. Play/Pause toggles music unless the app is a game, which may use it as
   pause; say so in the footer.
4. Lists: Up/Down move, Right/OK/Circle enter, Left/X back; Left at the app's
   first screen exits the app.
5. An app may override Left/Right/Up/Down/Triangle/Square/Circle/X on its
   own screens (Music now playing: Left/Right = previous/next). Document every
   override in this file and in the footer hint bar.
6. Act on press (value 1); Up/Down also on repeat (value 2); ignore release
   unless the app needs hold (Tetris).
7. Footer hints use the same words everywhere: `OK open`, `Lt/X back`,
   `Home top`, `Play play/pause`, `Vol volume`.

## 6. Volume

- `zpod-buttons` is the only writer of the DAC volume (`amixer Digital`),
  step 2 %, cap 80 % (≈ raw 204, just under 0 dB at 207), presses shorter
  than 75 ms ignored, repeat from 650 ms every 150 ms. Values in
  `/etc/zpod/buttons.conf`.
- It publishes `/run/zpod/volume` = `"<pct> <held>"`. The overlay
  ("Volume N%" + bar scaled to the cap) is shown from the first press,
  **stays while the key is held**, and hides 1.2 s after release.
- M3 bugs fixed: the overlay vanished during a hold (it timed out 1.5 s after
  the *first* event); the value jumped (kernel autorepeat plus relative
  `amixer 3%+` steps queued behind a slow `amixer`).

## 7. Display speed test (SPI 48/64 MHz)

The ILI9340 runs at **32 MHz** by default (safe). To try faster:

1. Settings > Display > Panel SPI speed → the screen redraws at 48 MHz.
2. If the menu looks perfect (no shifted columns, colour noise or white
   screen), press **OK within 10 s** to keep it; otherwise do nothing and it
   reverts to the previous speed.
3. Repeat for 64 MHz. Scroll Music lists and run Tetris for a minute to look
   for glitches.
4. The choice is stored in `/etc/zpod/ui.conf` `[display] spi_mhz`. If the
   panel is unreadable after a reboot, set `spi_mhz = 32` there over SSH
   (or `sudo systemctl edit zpod-launcher` → `Environment=ZPOD_SPI_MHZ=32`).

## 8. Battery page and low battery

See [../BATTERY.md](../BATTERY.md) for how the numbers are worked out.
Top menu > **Battery** (M6): big %, Charging / Charged / On battery, time
left (or time to full when charging), level bar, **Last charged** (when the
charger was last unplugged near full), **Full charge lasts** (median of
past full discharges, once there is enough history), cell voltage, and a
discharge chart since the last unplug. Without a fuel gauge it says so
instead.

M6: the Rust `zpod-battery` service (replaces the Python
`zpod-battery-guard`, same rule) reads the MAX17048 (confirmed at 0x36) every 30 s and
powers off cleanly when, on 3 reads in a row, SOC < 5 % **and** VCELL <
3.6 V **and** the gauge says it is not charging (CRATE ≤ 0). Change or
disable (`0`) in `/etc/zpod/ui.conf` `[battery] shutdown_pct`. The status
bar battery icon has 6 steps (empty outline in warning colour < 5 %, then
one bar per 20 %).

M6 also logs one reading a minute (time, SOC, VCELL, CRATE, charging) to
`/data/zpod/battery.log` (capped at 512 KiB, one rotation), which the
Battery page uses for its chart, "last charged" and "full charge lasts".
