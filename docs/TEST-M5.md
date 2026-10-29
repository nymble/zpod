# M5 test plan (`v0.6.0-m5-rc1`): Music, Tools, alarm/timer, file transfer, `/data`

Built and checked off-device (unit tests, simulator screenshots, image
verifier). **Nothing new in M5 has run on the ZPOD yet**; this is the first
hardware pass. Passwords are never written into this repo: where one is
needed you type it at the prompt.

Conventions: **Mac$** = the Mac (zsh), **Pi$** = the ZPOD over SSH
(`ssh paul@10.12.194.1` over USB or `ssh paul@zpod.local`).

## 0. Write the card and first boot

1. Write `zpod-pi0w-v0.6.0-m5-rc1.img.xz` with Raspberry Pi Imager (no OS
   customisation) or `xz -dc … | sudo dd …`. A **128 GB** card is the target.
2. Do the two-step first-boot setup from [FIRST-LOGIN.md](FIRST-LOGIN.md)
   (Step 1 asks for the password; Step 2 has none). The files are unchanged
   in M5.
3. Boot. First boot takes longer (it grows `/data`). Note the time to the
   menu.

Check:

```sh
Pi$ cat /etc/zpod-release                         # ZPOD_VERSION=0.6.0-m5-rc1
Pi$ lsblk -o NAME,SIZE,FSTYPE,LABEL,MOUNTPOINT /dev/mmcblk0
Pi$ findmnt /data /srv/zpod/media
Pi$ df -h / /data
Pi$ journalctl -b -u zpod-data-grow -u systemd-growfs@data -u zpod-data-setup --no-pager
Pi$ ls -ld /data/media/music /data/media/playlists /data/zpod/appdata
```

Expect: p3 `zpoddata` ≈ 115 GiB on `/data`; `/srv/zpod/media` is a bind of
`/data/media`; root ≈ 3 GiB and **not** grown; music owned by `paul`,
playlists `paul:mpd` 775, appdata `zpod`. Settings > Diagnostics > Storage &
build shows Root, **Data**, Music "on data", Boot and version 0.6.0-m5-rc1.

## 1. Menus

- [ ] Top menu: Music, Clock, Tetris, File transfer, **Tools >**, Settings >, Power >. Wi-Fi info is no longer at the top.
- [ ] Tools: Wi-Fi scanner, Bluetooth scanner, Wi-Fi info. Left/X back, Home top everywhere.
- [ ] Tone test still under Settings > Diagnostics; Play still play/pause everywhere; Vol keys still global.

## 2. Wi-Fi scanner (Tools)

- [ ] Opens and scans within ~5 s; your network is listed (cyan marker) with the right channel and security.
- [ ] Square = rescan (hint shows "scanning"), Triangle cycles sort: signal → name → channel.
- [ ] OK = details: SSID, BSSID, channel/MHz/band, dBm, security, "connected (this ZPOD)" for yours. Left back.
- [ ] Hidden networks show "(hidden)". WPA3/WPA2-WPA3 networks are labelled so (if you have one).
- [ ] Wi-Fi stays connected during and after scans (SSH session survives).

## 3. Bluetooth scanner (Tools)

Have a phone with Bluetooth settings open (discoverable) and any BLE device nearby.

- [ ] Opens, "Discovering devices (about 10 s)…", then a list with names, addresses, type (BR/EDR, LE public/random) and RSSI.
- [ ] Square rescans; Triangle sorts signal ↔ name; OK details.
- [ ] If it says "Scan failed: …", send `journalctl -b -u zpod-launcher --no-pager | tail -40` and `rfkill list; btmgmt info` (run `btmgmt info` with sudo).
- [ ] After a scan, `rfkill list` shows Bluetooth **not** soft-blocked (the launcher unblocks it).

## 4. Clock: alarm and timer

- [ ] Clock face: Up/Down change face; Square = seconds; Triangle = 12/24 h; OK/Right opens the Clock menu.
- [ ] Clock > Alarm: switch on, set Time (Up/Down, Right = minutes, OK saves), Repeat cycles Daily/Weekdays/Weekends/Once, Snooze cycles 5/9/10/15.
- [ ] **Test: ring in 5 s** → leave the Clock app (Home): the ring screen appears over the menu, a beep plays through the headphones at ≥ 60 % volume.
- [ ] Ring: Up = snooze (toast "Snoozed N min"); it rings again after N min; OK stops (toast "Alarm off").
- [ ] Start music, set the alarm 2 min ahead, open Tetris: at the alarm time Tetris closes, music pauses, beep plays; after stopping, music stays paused and plays again with Play.
- [ ] Vol keys during the ring change the volume and do **not** stop it. After stopping, the volume is back to what it was.
- [ ] Timer: Clock > Timer, set 0:30, OK starts; the face title shows "Timer 00:2x"; it rings "Time's up", any key stops.
- [ ] Reboot with the alarm on: the setting and time are kept (Clock > Alarm).
- [ ] Power off with the alarm set for 3 min later, power on after it: no ring (expected: **no wake from power-off**).

## 5. Music

- [ ] Artists: hold Down: after ~1 s it jumps letter by letter with a big letter shown; release stops.
- [ ] A long album track list: hold Down pages.
- [ ] Now Playing shows the `[Up] Repeat …` and `[Dn] Shuffle …` pills; Up/Down change them.
- [ ] Now Playing: Square → "Add track to playlist" → New playlist → toast "… added to Playlist 1"; Triangle adds the album.
- [ ] Music > Playlists > Playlist 1: Play, Shuffle play, Tracks work; Delete playlist asks, deletes, files stay.
- [ ] `Pi$ ls -l /srv/zpod/media/playlists/` shows the .m3u; it survives a reboot.

## 6. File transfer

- [ ] Screen shows USB IP, Wi-Fi IP + zpod.local, drop server state, rsync off, free space with "(/data)", song count + scan time, newest files.
- [ ] Copy an album: `Mac$ scripts/zpod-push-music.sh ~/Music/SomeAlbum`; the screen shows "rescanning…" then the new files under Newest files.
- [ ] OK = rescan works (toast, then "rescanning…").
- [ ] Square turns the rsync share on (toast "rsync share on (USB only)"), then `Mac$ rsync rsync://10.12.194.1/music/` lists the library and `Mac$ rsync -rtv ~/Music/SomeAlbum rsync://10.12.194.1/music/` uploads; Square again turns it off and the same command is refused.
- [ ] Over Wi-Fi `rsync rsync://zpod.local/music/` is refused even when the share is on.

## 7. Regression

- [ ] Boot time to the menu (M4.1 was ~1:30): ________
- [ ] USB networking (NCM), Wi-Fi via `zpod-wifi.txt`, battery, RTC time, shutdown screen, low-battery settings unchanged.
- [ ] `Pi$ systemctl --failed` is empty. `Pi$ man ls` says no manual entry (expected: man pages trimmed).
- [ ] `Pi$ sudo apt update && apt list --upgradable` works (apt lists were removed from the image).

## 8. Report back

Ticks/crosses for each section, the boot time, photos of anything odd, and
`Pi$ journalctl -b --no-pager -p warning | tail -80`.
