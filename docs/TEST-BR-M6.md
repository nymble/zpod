# Hardware test: Buildroot fast-boot M6 test image (`v0.8.0-br-m6-fastboot-test1`)

**Spare card only** (Paul's older 32 GB card). Do not use the M5 music card:
writing the image erases the card. Mac commands are zsh; `Pi$` is on the ZPOD
over SSH. Design and background: [BUILDROOT.md](./BUILDROOT.md).

## 0. Write and prepare the card (before the first power-on)

1. Raspberry Pi Imager → "Use custom" → `2026-10-30-zpod-pi0w-v0.8.0-br-m6-fastboot-test1.img.xz`
   (in `~/Downloads`) → the spare card. No Imager customisation (it is ignored).
2. Re-insert the card: it mounts as **`/Volumes/ZPODBOOT`**. Put the
   first-boot files on it:

```zsh
H=$(openssl passwd -6) && case "$H" in '$6$'*) printf 'paul:%s\n' "$H" > /Volumes/ZPODBOOT/userconf.txt ;; *) echo "use OpenSSL 3 (conda/Homebrew); macOS openssl has no -6"; false ;; esac; unset H
cp ~/.ssh/id_ed25519.pub /Volumes/ZPODBOOT/zpod-ssh.pub
printf 'SSID=%s\nPSK=%s\n' '<SSID>' '<PW>' > /Volumes/ZPODBOOT/zpod-wifi.txt
ls /Volumes/ZPODBOOT/userconf.txt /Volumes/ZPODBOOT/zpod-ssh.pub /Volumes/ZPODBOOT/zpod-wifi.txt
diskutil eject /Volumes/ZPODBOOT
```

## 1. Boot time (the main point)

- [ ] Film the screen and the power switch together. Power on.
- [ ] Expected on the 320x240 panel: the **ZPOD boot splash** (logo + progress
      bar) after ~2–3 s, then the **launcher home screen**: Music, Clock,
      Tetris and System with icons, status bar (time, battery %, Wi-Fi bars,
      USB badge, volume). Estimate: **~5–6 s from power-on** (not yet measured).
- [ ] Record: power switch → splash, power switch → home screen (from the video).
- [ ] `Pi$ zpod-boottime` (connect USB first, see 3). Note `launcher-exec` and
      `launcher-first-frame`; add ~2.3–2.5 s of GPU firmware for power-on time.

If the panel stays black or white: wait 30 s, then try SSH (3); send
`/run/zpod/launcher.log` and `/run/zpod/bootsplash.log`.

## 2. Display, buttons, audio

- [ ] Panel orientation and colours right (red is red), no garbage.
- [ ] D-pad, OK (centre), X, Circle, Square, Triangle, Home, Play work in the menu.
- [ ] Vol+/Vol- show the "Vol NN%" toast and change the volume.
- [ ] Music plays through the headphones (push a track first, see 5); no hiss
      or mute (GPIO14 LOW): `Pi$ cat /proc/asound/cards` lists `sndrpiaoidezpod`.
- [ ] Settings/System > Status: battery % and volts plausible (MAX17048 @0x36).
- [ ] `Pi$ hwclock -r` shows the right time (DS3231 @0x68); `date` too.

## 3. USB, login, sudo

- [ ] USB cable on the data port: the Mac gets 10.12.194.x (System Settings >
      Network shows a new USB Ethernet (NCM) interface).
- [ ] `ssh paul@10.12.194.1` with the key works; with the password too.
- [ ] `ssh root@10.12.194.1` works with the key; **password login for root is refused**.
- [ ] `Pi$ sudo -v` asks for paul's password and works.
- [ ] `Pi$ ls /boot/firmware`: `userconf.txt`, `zpod-ssh.pub`, `zpod-wifi.txt` are gone.
- [ ] `Pi$ cat /data/log/firstuser.log`
- [ ] System > Diagnostics > Storage & build: Login row shows `paul`.

## 4. /data grows to the card

- [ ] `Pi$ df -h /data /srv/zpod/media`: ~28–29 GB on a 32 GB card.
- [ ] `Pi$ cat /data/log/data-grow.log` (sfdisk/partx/resize2fs lines).
- [ ] `Pi$ grep srv/zpod/media /proc/mounts` (bind mount of /data/media).
- [ ] Reboot: still the same size; data-grow.log has nothing new.

## 5. Wi-Fi, rsync

- [ ] `Pi$ cat /run/zpod/wifi.log`; `ip -4 addr show wlan0` has an address.
- [ ] `Pi$ grep -c psk= /data/etc/wpa_supplicant.conf` is 1 and the file has no
      readable password (only `psk=<64 hex>`).
- [ ] Tools > Wi-Fi scanner lists networks; Wi-Fi info shows SSID/signal/IP.
- [ ] `scripts/zpod-push-music.sh ~/Music/<Album>` (rsync over SSH) works; the
      album shows up in Music after the rescan.
- [ ] File transfer screen: turn the rsync share on, then from the Mac
      `rsync -rtv ~/Music/<Album> rsync://10.12.194.1/music/`; turn it off.
- [ ] `ntpd` set the time: `Pi$ date` correct with Wi-Fi up.

## 6. Bluetooth

- [ ] `Pi$ cat /run/zpod/bt.log; ls /sys/class/bluetooth` shows `hci0`.
- [ ] `Pi$ btmgmt info` shows the controller; Tools > Bluetooth scanner lists
      nearby devices.
- [ ] Play music while the Bluetooth scanner runs: no mute or hiss (GPIO14
      stays LOW; Bluetooth is on the internal PL011, GPIO30-33).

## 7. Power

- [ ] System > Shutdown: end screen, then safe to switch off. Power on again: boots.
- [ ] Reboot from the menu works.

Report on [#22](https://github.com/nymble/zpod/issues/22): the video times,
`zpod-boottime`, and any failed box with its log (`/run/zpod/*.log`,
`/data/log/*.log`, `dmesg | tail -50`).
