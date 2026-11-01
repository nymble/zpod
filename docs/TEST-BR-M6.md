# Hardware test: Buildroot fast-boot M6 test image (`v0.9.0-br-m6-fastboot-test3`)

test3 = test2 + the fixes from Paul's test2 run and the new Tools / Wi-Fi
layout. **Do section T3 first**; sections 1-8 are the test2 checklist, still
valid (menu paths updated). Draft PR [#23](https://github.com/nymble/zpod/pull/23),
issue [#22](https://github.com/nymble/zpod/issues/22).

**Spare card only** (Paul's older 32 GB card). Do not use the M5 music card:
writing the image erases the card. Mac commands are zsh; `Pi$` is on the ZPOD
over SSH. Design and background: [BUILDROOT.md](./BUILDROOT.md).

## 0. Write and prepare the card (before the first power-on)

1. Raspberry Pi Imager → "Use custom" → `2026-11-01-zpod-pi0w-v0.9.0-br-m6-fastboot-test3.img.xz`
   (in `~/Downloads`) → the spare card. No Imager customisation (it is ignored).
   Check it first: `Mac$ shasum -a 256 ~/Downloads/2026-11-01-zpod-pi0w-v0.9.0-br-m6-fastboot-test3.img.xz`
   = `7170ec785c19264c638297845a76d9ded94b9b4f179550eacd0ac929b010e90e`
   (87,597,068 bytes; the uncompressed `.img` is
   `432974ae23ce90735581e03222409b798da9c0c80c53c24ad00fc194fb91c250`, 872,415,744 bytes).
2. Re-insert the card: it mounts as **`/Volumes/ZPODBOOT`**. Put the
   first-boot files on it:

```zsh
H=$(openssl passwd -6) && case "$H" in '$6$'*) printf 'paul:%s\n' "$H" > /Volumes/ZPODBOOT/userconf.txt ;; *) echo "use OpenSSL 3 (conda/Homebrew); macOS openssl has no -6"; false ;; esac; unset H
cp ~/.ssh/id_ed25519.pub /Volumes/ZPODBOOT/zpod-ssh.pub
printf 'SSID=%s\nPSK=%s\n' '<SSID>' '<PW>' > /Volumes/ZPODBOOT/zpod-wifi.txt
ls /Volumes/ZPODBOOT/userconf.txt /Volumes/ZPODBOOT/zpod-ssh.pub /Volumes/ZPODBOOT/zpod-wifi.txt
diskutil eject /Volumes/ZPODBOOT
```

## T3. What is new in test3 (what success looks like on screen)

Menu layout (screenshots: `screenshots/t3-*.png`, sim):

- [ ] **Top menu** = Music, Clock, Tetris, **Tools >**, Settings >, Power >
      (Battery and File transfer are no longer on the top menu).
- [ ] **Tools** = **Wi-Fi, Bluetooth, USB, Battery, File transfer**, in that order.
- [ ] **Play on a Tools screen with nothing playing: nothing happens** (no
      "Nothing queued" toast). With music playing, Play on a Tools screen
      pauses it; on the top menu Play still toggles play/pause.

Bug fixes from test2:

- [ ] `Pi$ timeout 1 sleep 5; echo $?` prints `124` (BusyBox `timeout` is in).
- [ ] Tools > Wi-Fi > Networks nearby lists networks (test2: every scan
      failed because `timeout` was missing).
- [ ] Tools > **Bluetooth** lists devices with names and RSSI (test2 showed
      nothing; `bluetoothctl` saw ~20 at −45 to −83 dBm). The hint line shows
      the source (`btmgmt find` or `bluetoothctl`).
- [ ] **Red Play LED**: **off after boot** with nothing playing; on while
      music plays; off again a second or two after Stop (it may stay on while
      paused). If it is still on after boot, report it with
      `Pi$ modinfo aoide-zpod-dac | head -3` and `dmesg | grep -i -e aoide -e pcm512`. (Driver fix, see
      `drivers/aoide-zpod-dac/README.md`; not confirmed on hardware yet.)

Wi-Fi (one screen, Tools > Wi-Fi):

- [ ] The top **status card** shows a mode pill (**Client** cyan), the SSID,
      `IP   ch N`, `-NN dBm good  NN Mbit/s` and the MAC. Rows: **Mode >,
      Networks nearby >, Saved networks N >, Wi-Fi tools >**.
- [ ] Status bar Wi-Fi icon: 0-4 white bars in Client mode.
- [ ] Networks nearby > OK on a network: details with the **live meter**
      (the standalone Signal meter is gone). On an **open** network OK =
      join (it is saved); on a network with a password the toast says to add
      it with zpod-wifi.txt (no on-screen keyboard yet); on a saved one OK
      connects.
- [ ] Saved networks: OK = details/meter; **Triangle, Triangle** = forget
      (`Pi$ grep -c '^network=' /data/etc/wpa_supplicant.conf` drops by one).
      Don't forget your home network unless you have zpod-wifi.txt handy.
- [ ] **Mode > Hotspot**: card turns amber "Hotspot ZPOD-xxxx", `10.42.0.1
      ch N`, "0 devices connected"; status bar shows the antenna + **0**.
      Join from the iPad: within ~5 s the count is **1** (status bar and
      card); **Connected devices** lists it (name or IP, MAC, time, dBm).
- [ ] **Password**: dots until OK (show/hide); Triangle = new password
      (the iPad must re-join). `Pi$ ls -l /data/etc/NetworkManager/system-connections/`
      shows the profile as `-rw-------` root.
- [ ] On the iPad open `http://10.42.0.1:8080/` with Wi-Fi tools > Drop box on.
- [ ] **Mode > Off**: card "Off  Wi-Fi is off", status bar icon struck
      through; `Pi$ pgrep -l 'hostapd|wpa_supplicant|udhcpd|udhcpc'` shows
      only the USB `udhcpd` (no wlan0 processes); `ip link show wlan0` is DOWN.
      Reboot: still Off (`/data/etc/zpod/wifi-off`).
- [ ] **Mode > Client** (from Off or Hotspot): back on the home network within
      ~30 s; card shows SSID/IP again; no `hostapd` left (`pgrep hostapd`).
- [ ] Wi-Fi tools > **Channel utilization**: axis 1-13, one arch per network
      (about 4 channels wide, taller = stronger, yours cyan, strongest six
      labelled), shaded busy columns, a **green triangle under the least
      busy of 1/6/11**. OK = rescan.
- [ ] Wi-Fi tools > **Devices & addresses**: "N on this LAN (ARP) · M
      access points"; LAN rows (IP, MAC) then access points (BSSID, ch,
      security, dBm). Passive only.

USB:

- [ ] Tools > **USB** with the Mac connected: green **Connected**, `CDC-NCM`,
      Controller `configured  high-speed`, usb0 `up 10.12.194.1`, Host
      `10.12.194.x  <mac>`, host name if the Mac sent one, traffic counters.
      Unplug: "Not connected" within ~3 s.

## 1. Boot time (the main point)

- [ ] Film the screen and the power switch together. Power on.
- [ ] Expected on the 320x240 panel: the **ZPOD boot splash** (logo + progress
      bar) after ~2–3 s, then the **launcher home screen**: Music, Clock,
      Tetris and System with icons (core 0.9.0 / M6), status bar (time, battery %, Wi-Fi bars,
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
- [ ] Tools > Wi-Fi > Networks nearby lists networks; the status card shows SSID/signal/IP.
- [ ] `scripts/zpod-push-music.sh ~/Music/<Album>` (rsync over SSH) works; the
      album shows up in Music after the rescan.
- [ ] File transfer screen: turn the rsync share on, then from the Mac
      `rsync -rtv ~/Music/<Album> rsync://10.12.194.1/music/`; turn it off.
- [ ] `ntpd` set the time: `Pi$ date` correct with Wi-Fi up.

## 6. Bluetooth

- [ ] `Pi$ cat /run/zpod/bt.log; ls /sys/class/bluetooth` shows `hci0`.
- [ ] `Pi$ btmgmt info` shows the controller; Tools > Bluetooth lists
      nearby devices.
- [ ] Play music while the Bluetooth scanner runs: no mute or hiss (GPIO14
      stays LOW; Bluetooth is on the internal PL011, GPIO30-33).

## 7. M6 apps (Buildroot backends)

- [ ] Settings > About: version `v0.9.0-br-m6-fastboot-test3`, git commit and
      build date; core 0.9.0 shown separately.
- [ ] Tools > Battery: % / volts / rate; after a few minutes
      `Pi$ tail /data/zpod/battery.log` has one line per minute.
- [ ] Wi-Fi > Networks nearby > your network: the meter follows the signal when you walk away.
- [ ] Wi-Fi > Saved networks lists the `zpod-wifi.txt` network as
      active; OK > OK on it reconnects (`Pi$ wpa_cli status`).
- [ ] Mode > Hotspot: a phone sees `ZPOD-xxxx`, joins with the shown password and
      gets 10.42.0.x; the client count goes to 1. `Pi$ cat /run/zpod/hostapd.log`.
- [ ] With the hotspot on, Wi-Fi tools > Drop box on: the phone opens
      `http://10.42.0.1:8080/` and uploads a file (lands in `/data/inbox` or
      music); Book catalog on: an OPDS reader (KOReader) lists `/data/books`.
- [ ] Mode > Client: the ZPOD rejoins home Wi-Fi within ~30 s (`ip -4 addr show wlan0`).
- [ ] Drop box/Book catalog off again; after a reboot both are still off
      (`Pi$ systemctl is-active zpod-drop.service` → inactive).
- [ ] Boot notice: copy a second public key to ZPODBOOT as `zpod-ssh.pub`
      and boot: the launcher's first screen says "zpod-ssh.pub: 1 new SSH
      key(s) for paul" (and for root); OK dismisses it.

## 8. Power

- [ ] Power > Shut down: end screen, then safe to switch off. Power on again: boots.
- [ ] Reboot from the menu works.

Report on [#22](https://github.com/nymble/zpod/issues/22): the video times,
`zpod-boottime`, and any failed box with its log (`/run/zpod/*.log`,
`/data/log/*.log`, `dmesg | tail -50`).
