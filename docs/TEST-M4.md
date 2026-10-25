# M4 test plan: apps, music, transfer, splash

Everything here was built and simulated on the build box only. **None of it
has run on the ZPOD yet.** This plan is the first hardware run. Passwords are
shown as `********`; never write them into files in this repo.

Conventions: **Mac$** runs on the Mac in the repo checkout. **Pi$** runs on the
ZPOD (`ssh pi@10.12.194.1`, password ********). The USB cable gives the Pi
10.12.194.1.

## 0. Build the artifacts (build host)

```sh
cd apps
tools/check-nostd.sh                 # no_std cores build for thumbv7em-none-eabihf
cargo test --workspace               # host unit tests
tools/build-arm.sh                   # ARMv6 static binaries + dist/packages/*.zpa; prints Tag_CPU_arch (must be v6)
```

Copy `apps/dist` to the Mac (or build there with the same commands; needs
rustup target `arm-unknown-linux-musleabihf`, cargo-zigbuild and zig).

## 1. Install on an M3 image without reflashing (reversible)

```sh
Mac$ scp -r apps/dist/core/usr/lib/zpod/bin apps/system pi@10.12.194.1:/tmp/m4/
Mac$ scp apps/dist/packages/*.zpa pi@10.12.194.1:/tmp/m4/
Mac$ ssh -t pi@10.12.194.1
Pi$  sudo apt-get install -y mpd mpc rsync            # Wi-Fi needed once
Pi$  sudo install -d /usr/lib/zpod/bin /opt/zpod/apps /etc/zpod
Pi$  sudo install -m755 /tmp/m4/bin/* /usr/lib/zpod/bin/
Pi$  sudo ln -sf /usr/lib/zpod/bin/zpod-app /usr/local/bin/zpod-app
Pi$  sudo ln -sf /usr/lib/zpod/bin/zpod-bootsplash /usr/local/bin/zpod-bootsplash
Pi$  sudo install -m644 /tmp/m4/system/systemd/* /etc/systemd/system/
Pi$  sudo install -m644 /tmp/m4/system/etc/zpod/core-version /tmp/m4/system/etc/zpod/drop.conf /etc/zpod/
Pi$  sudo cp /etc/mpd.conf /etc/mpd.conf.dist && sudo install -m644 /tmp/m4/system/etc/mpd/mpd.conf /etc/mpd.conf
Pi$  sudo useradd --system --home-dir /var/lib/zpod/home --create-home --shell /usr/sbin/nologin zpod
Pi$  for g in spi gpio i2c input audio video; do sudo usermod -aG $g zpod; done
Pi$  sudo install -d -o 1000 -g 1000 -m 0775 /srv/zpod/media /srv/zpod/media/music /srv/zpod/media/playlists
Pi$  sudo usermod -aG pi mpd
Pi$  for p in /tmp/m4/*.zpa; do sudo zpod-app install "$p"; done; zpod-app list
Pi$  sudo systemctl daemon-reload && sudo systemctl restart mpd
```

Expected: `zpod-app list` shows music, clock and tetris at 1.0.0. Any
"Tag_CPU_arch"/"64-bit" refusal means a wrong build target.

Rollback at any point:
`sudo systemctl disable --now zpod-launcher zpod-bootsplash zpod-drop.socket; sudo systemctl enable --now zpod-ui`.

## 2. Panel driver (mipidsi) and launcher, by hand first

```sh
Pi$ sudo systemctl stop zpod-ui
Pi$ sudo rm -f /run/zpod/lcd-initialised          # force the cold init path
Pi$ sudo /usr/lib/zpod/bin/zpod-launcher          # Ctrl-C to quit
```

Check:

* [ ] Panel initialises: correct orientation (not mirrored), colours right
  (red is red: BGR via MADCTL 0xE8), no garbage. This is the key check that
  the mipidsi model reproduces MADCTL 0xE8 with no reset pin and DC=GPIO25.
* [ ] Home lists Music, Clock, Tetris and System with icons. Up/Down/OK work,
  and so do OK = centre (all four), X and Home.
* [ ] Status bar: time, battery %, Wi-Fi bars, USB badge, volume.
* [ ] Ctrl-C (SIGINT) leaves a blank panel with the backlight off
  (`screen = message` applies only to poweroff).
* [ ] Run it again: no SWRESET flash (warm init, marker present).

## 3. Apps and the hand-off

* [ ] Open Clock: the launch card appears, then the clock with no white flash.
  Left/Right cycles three faces; OK toggles seconds; Triangle toggles 12/24 h;
  Home returns. CPU while idle: `top -d 5` shows clock under 1 %.
* [ ] Open Tetris: play, Circle/Triangle rotate, Square hold, Up hard drop,
  Down soft drop, Play pause, Home exits and saves the high score.
  Re-enter: BEST is kept.
* [ ] Hung-app escape: `Pi$ sudo kill -STOP $(pgrep -x tetris)`, then hold
  Home for 3 s. The launcher kills it and shows "Tetris force quit".
* [ ] Apps run as `zpod`: `ps -o user,comm -C tetris` shows zpod.
* [ ] Backlight across hand-offs: no blink when the launcher releases GPIO27
  (relies on the RPi kernel keeping GPIO outputs on release; if it blinks,
  report it).

## 4. System screens (parity with the Python zpod-ui)

* [ ] Status (values plausible; battery volts vs `i2cget -y 1 0x36 0x02 w`).
* [ ] Volume: Up/Right +3 %, Down/Left -3 %; matches `amixer -c sndrpiaoidezpod sget Digital`.
* [ ] Vol+/Vol- anywhere: one step per press (the launcher must not double-apply), with a "Vol NN%" toast.
* [ ] Wi-Fi info; OK refreshes.
* [ ] Play test tone: 440 Hz for 10 s; selecting it again stops it. With
  MPD playing, MPD pauses first. If the tone fails with "device busy", note
  it (MPD holds `hw:` while paused).
* [ ] Reboot: confirm, "Rebooting..." stays until power-cycle of the panel.
* [ ] Shutdown with `/etc/zpod/ui.conf` `screen = message`: the final "ZPOD off /
  Safe to power off / Halted <date>" frame stays lit after halt.
* [ ] Shutdown with `screen = off`: black panel and backlight off.

## 5. Music

```sh
Mac$ scripts/zpod-push-music.sh ~/Music/"Some Artist"
Pi$  mpc stats                                   # artists/albums/songs counted
```

* [ ] Artists, then albums, then tracks; OK plays. Now Playing shows art from
  cover.jpg, embedded art (MP3/FLAC picture), or a note icon when there is
  none.
* [ ] Up = repeat cycle, Down = shuffle, Left/Right = prev/next, hold Right
  seeks, OK opens the queue, Square adds to the queue, Play pauses.
* [ ] Home goes to the launcher; music keeps playing. Play/Pause on the
  launcher toggles MPD.
* [ ] Gapless across tracks of one album.
* [ ] CPU while playing FLAC with the UI on Now Playing: `top` (record mpd% and music%).
* [ ] Volume: Vol+/- changes loudness (zpod-buttons sets ALSA 'Digital'; MPD has no mixer).

## 6. File transfer

```sh
Mac$ time scripts/zpod-push-music.sh ~/Music/"Album"            # USB: record MB/s
Mac$ time scripts/zpod-push-music.sh -H zpod.local ~/Music/"Album"   # Wi-Fi
Mac$ open http://10.12.194.1:8080/                             # drop page; upload a folder
Mac$ curl -T test.flac "http://10.12.194.1:8080/music/Test/Test/test.flac"
Mac$ curl -s http://10.12.194.1:8080/status
Mac$ curl -s -o /dev/null -w '%{http_code}\n' -T test.flac "http://zpod.local:8080/music/x/y.flac"   # expect 403 (no token)
Mac$ curl -s -o /dev/null -w '%{http_code}\n' -T evil.sh "http://10.12.194.1:8080/music/../../x.mp3"  # expect 400
```

* [ ] Accented names (e.g. "Sigur Fjörd") appear once in Artists (NFC).
* [ ] Pull the USB cable mid-upload: no partial file in the library (only `.x.part`, removed on failure).
* [ ] `systemctl status zpod-drop` goes inactive about 10 min after the last use.

## 7. Boot splash timing

```sh
Pi$ sudo systemctl enable zpod-bootsplash zpod-launcher zpod-drop.socket && sudo systemctl disable zpod-ui
Pi$ sudo reboot
# after boot:
Pi$ cat /run/zpod/splash-first-frame          # seconds after kernel start at the first splash frame
Pi$ journalctl -b -u zpod-bootsplash -u zpod-launcher -o short-monotonic | head
Pi$ systemd-analyze; systemd-analyze critical-chain zpod-launcher.service
```

* [ ] Record: power-on to first splash pixels (stopwatch), the
  `splash-first-frame` value, and launcher start time. Compare with M3, where
  the zpod-ui first frame came after multi-user.
* [ ] Status lines stream ("Starting ...", "Started ..."); the progress bar
  moves; the hand-off to the launcher has no flash.
* [ ] Second boot: the progress bar is calibrated (reaches ~90-97 % near the hand-off).

## 8. Report back

Note the binary sizes from `build-arm.sh`, the timings from step 7, the CPU
numbers from step 5, the transfer speeds from step 6, and every unchecked box.

## 9. M4 candidate image (fixes from the 2026-10-22 test)

Image: `~/zpod-build/deploy/m4/` on the Mac (not published). Write it to a
card yourself, add `userconf.txt` and (optionally) `zpod-wifi.txt` as for
v0.4.1. **Without `userconf.txt` no login works** (SSH shows the rptl.io/newuser
banner): see [FIRST-LOGIN.md](./FIRST-LOGIN.md), which also covers
`zpod-ssh.pub` (M4.1+). Button and menu spec: [ui/BUTTONS-AND-MENUS.md](./ui/BUTTONS-AND-MENUS.md).

| # | Check | Pass |
| --- | --- | --- |
| 9.1 | USB: data cable Mac ↔ Pi `USB` port, no Mac settings changed | within ~90 s an interface "ZPOD USB Ethernet" with a 10.12.194.x address; `ping 10.12.194.1` and `ssh <user>@10.12.194.1` work and **keep working for 5 minutes** (`ping -c 300`) |
| 9.2 | USB, if 9.1 fails | report `system_profiler SPUSBDataType \| grep -A8 -i zpod` and `netstat -I enX -b`; then try `mode=ecm` in `zpod-usb.txt` on the boot volume |
| 9.3 | Volume: hold Vol+ 3 s | overlay appears at once, **stays while held**, number climbs 2 % at a time with no jumps, stops at 80 %; hides ~1 s after release |
| 9.4 | Volume: quick tap | exactly one 2 % step |
| 9.5 | Home from any screen/app | top menu |
| 9.6 | Left / X | back one level in Settings > Display, Diagnostics, Power |
| 9.7 | Play/Pause | music play/pause (or "Nothing queued" toast); never the tone |
| 9.8 | Settings > Diagnostics > Tone test | 440 Hz tone; Play or the item again stops it |
| 9.9 | Wi-Fi info | SSID, signal, IP, **MAC** (matches `ip link show wlan0`) |
| 9.10 | Clock app (menu) | 3 faces with Up/Down/Right, OK = seconds, Left exits |
| 9.11 | RTC: Settings > Date & time → set, OK; shut down; boot again away from Wi-Fi | time is right without network; `sudo hwclock -r` matches |
| 9.12 | Settings > Display > Backlight test | screen goes dark for 3 s → GPIO27 **is** the backlight (report yes/no) |
| 9.13 | Settings > Display > Panel SPI speed | 48 then 64 MHz: report whether the screen stays clean; press OK only if it does |
| 9.14 | Battery & about | battery %, "shuts down below 5%", RTC time |
| 9.15 | Low battery (optional, long) | run on battery until it powers itself off cleanly ("ZPOD off" screen) |
| 9.16 | Button test (Diagnostics) | every button lights its box; Home exits |

Logs: `journalctl -b -u zpod-usb-gadget -u zpod-usb-kick -u zpod-buttons -u zpod-launcher -u zpod-rtc -u zpod-battery-guard`.

## 10. M4.1 candidate (login setup, storage & build screens)

Write the card, then on bootfs: `userconf.txt` (password) and/or
`zpod-ssh.pub` (+ optional `zpod-user.txt`), see [FIRST-LOGIN.md](./FIRST-LOGIN.md).

| # | Check | Pass |
| --- | --- | --- |
| 10.1 | `userconf.txt` + `zpod-ssh.pub` | `ssh -o BatchMode=yes paul@zpod.local true` works (key); password works too; `ls /boot/firmware` shows neither file |
| 10.2 | Key-only (no `userconf.txt`, `zpod-user.txt` = paul) on a fresh card | key login as paul; `sudo -n true` works; no SSH banner |
| 10.3 | Settings > Battery & about | Build row = version, git commit, build date; SD card row; Host row shows `login paul` |
| 10.4 | Settings > Diagnostics > Storage & build | values match `cat /etc/zpod-release` and `df -h / /srv/zpod/media /boot/firmware`; SD card size matches `lsblk -b /dev/mmcblk0` |
| 10.5 | Diagnostics > System status | SD card + Build rows present |
| 10.6 | Music push | `scripts/zpod-push-music.sh -u paul ~/Music/...` then `mpc stats` counts the albums |

