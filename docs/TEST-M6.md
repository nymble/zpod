# M6 test plan (`v0.9.0-m6-rc1`): Battery, Wi-Fi tools, hotspot, drop box, book catalog, fast boot

Built and checked off-device (unit tests, simulator screenshots in
`screenshots/m6-*.png`, image verifier, a QEMU boot-time comparison).
**Nothing new in M6 has run on the ZPOD yet**; this is the first hardware
pass. Passwords are never written into this repo: where one is needed you
type it at the prompt.

Conventions: **Mac$** = the Mac (zsh), **Pi$** = the ZPOD over SSH
(`ssh paul@10.12.194.1` over USB or `ssh paul@zpod.local`).

Version note: the Buildroot line already uses `v0.7.0-br-poc1` and
`v0.8.0-br-m6-fastboot-test1`, so this Pi OS release is **0.9.0**
(`v0.9.0-m6-rc1`) to keep the two lines apart.

## 0. Write the card and first boot

1. Write `2026-10-30-zpod-pi0w-v0.9.0-m6-rc1.img.xz` with Raspberry Pi Imager
   (no OS customisation) or `xz -dc … | sudo dd …`.
2. Put **all** bootfs files on the card before the first power-on
   ([FIRST-LOGIN.md](FIRST-LOGIN.md), Quick setup: `userconf.txt`,
   `zpod-ssh.pub`, `zpod-wifi.txt` with your `<SSID>`/`<PW>`).
3. Boot. The first boot is longer (it grows `/data`, makes SSH host keys).
4. **Time the second boot** (power switch on → menu drawn), twice: ______ / ______
   (M5: 1:09. Target: 35-45 s.)

```sh
Pi$ cat /etc/zpod-release                     # ZPOD_VERSION=0.9.0-m6-rc1
Pi$ systemd-analyze; systemd-analyze blame | head -20
Pi$ systemd-analyze critical-chain zpod-launcher.service
Pi$ systemctl --failed
Pi$ ls -ld /data/books /data/inbox            # paul, 775
```

## 1. Menus

- [ ] Top: apps…, File transfer, **Battery**, Tools >, Settings >, Power >.
- [ ] Settings > **About**: version, SD card size + use per partition, host/login, RTC, kernel, core 0.9.0. No battery on it.
- [ ] Tools: Wi-Fi scanner, Signal meter, Channels, Saved networks, Hotspot, Drop box, Book catalog, Bluetooth scanner, Wi-Fi info.

## 2. Battery

- [ ] Battery page: big %, "On battery" / "Charging" / "Charged", time left (on battery, after a few minutes), cell voltage.
- [ ] Plug in the charger: within ~1 min it says Charging (CRATE > 0). Unplug: On battery.
- [ ] `Pi$ tail -3 /data/zpod/battery.log` grows by one line a minute: `<unix> <soc> <V> <%/h> <0|1>`.
- [ ] After a long discharge and a recharge: "Last charged" and "Full charge lasts" fill in; the chart shows the discharge. `Pi$ cat /data/zpod/battery-sessions.log`.
- [ ] `Pi$ systemctl status zpod-battery` active; `systemctl is-enabled zpod-battery-guard` = disabled.
- [ ] Low battery (optional, long): still powers off cleanly below 5 % (journal: "zpod-battery: battery empty").

## 3. Wi-Fi tools

- [ ] Signal meter: dBm + quality updates every second; walk away from the router and back, the graph follows.
- [ ] Wi-Fi scanner > a network > details > **OK** opens the meter for that network (slower updates).
- [ ] Channels: networks per channel 1-13, "least busy: ch N"; Square rescans.
- [ ] Saved networks: lists your profiles (the one from `zpod-wifi.txt` is `zpod-wifi`); OK on another saved one switches (if you have two). SSH over USB survives.

## 4. Hotspot

- [ ] Tools > Hotspot shows "Hotspot off" and warns it **leaves your home Wi-Fi**.
- [ ] OK: within ~20 s shows `ZPOD-xxxx`, a 16-character password, `10.42.0.1`, channel. Write nothing down in the repo.
- [ ] Phone joins `ZPOD-xxxx` with that password; client count goes to 1.
- [ ] `Pi$ sudo ls -l /etc/NetworkManager/system-connections/zpod-hotspot.nmconnection` is `-rw------- root`.
- [ ] Triangle = new password; the phone must re-join with it.
- [ ] OK again turns it off and the ZPOD reconnects to the home Wi-Fi (Wi-Fi info shows your SSID).

## 5. Drop box

- [ ] Off by default: `Mac$ curl -m 3 http://10.12.194.1:8080/` fails (connection refused).
- [ ] Tools > Drop box > OK: "Drop box on" with URLs. Open `http://10.12.194.1:8080/` on the Mac: Music, Books, Inbox lists; download a song.
- [ ] Upload a file from the page: it lands in `/data/inbox`; a second upload with the same name becomes `name (2).ext`.
- [ ] From a phone on the ZPOD hotspot: `http://10.42.0.1:8080/` works without a token.
- [ ] On home Wi-Fi without a token in `/etc/zpod/drop.conf`: refused (403).
- [ ] OK again: off; the curl above fails again.

## 6. Book catalog (OPDS)

- [ ] `Mac$ scp some.epub paul@10.12.194.1:/data/books/`
- [ ] Tools > Book catalog > OK. `Mac$ curl -s http://10.12.194.1:8080/opds | head` is an Atom feed.
- [ ] In KOReader / Thorium / Marvin (on the hotspot): add `http://10.42.0.1:8080/opds`; All books shows the title, author and cover; download opens the book.
- [ ] Drop box off + catalog on: `/` says the drop box is off; `/opds` works.

## 7. Boot notice (files added after the first boot)

- [ ] Power off, put a new `zpod-wifi.txt` (or `zpod-ssh.pub`) on bootfs, boot: the first screen is "Setup files used at this boot" with one line per file. OK dismisses; the next boot goes straight to the menu.
- [ ] A stray `userconf.txt` on a set-up card: "userconf.txt ignored: login already set up".

## 8. Music push permissions (fix)

- [ ] `Mac$ scripts/zpod-push-music.sh ~/Music/SomeAlbum` ends with "permissions fixed (dirs 755, files 644)"; `Pi$ ls -l "/srv/zpod/media/music/SomeAlbum"` shows `-rw-r--r--`; MPD plays it.

## 9. Regression (fast-boot trims)

- [ ] SSH over USB (`10.12.194.1`) and over Wi-Fi work; `zpod-wifi.txt` imported on first boot.
- [ ] Audio (DAC), buttons, volume, LCD, RTC time, shutdown screen unchanged.
- [ ] `/data` grew at first boot (`df -h /data`).
- [ ] Nothing in `systemctl --failed`. No HDMI output is expected any more (KMS removed).

## 10. Report back

Ticks/crosses, both boot times, photos of anything odd, and
`Pi$ systemd-analyze blame | head -25; journalctl -b -p warning --no-pager | tail -60`.
