# M3 on-device test checklist (`v0.4.1-m3`; `v0.4.0-m3` withdrawn)

Board: Pi Zero W in the ZPOD case. Image: `2026-10-18-zpod-pi0w-m3.img.xz` (`v0.4.1-m3`;
same as the withdrawn `v0.4.0-m3` image, minus the embedded Wi-Fi profile, plus first-boot Wi-Fi import)
(check it against `SHA256SUMS` first). Never flash a card you care about.
M3 = M2 (confirmed on hardware 2026-10-17, see [BUILD-RECORD](./BUILD-RECORD.md))
plus the fixes found on hardware and boot-time trims.

## Hardware results (Paul, 2026-10-17, `v0.4.0-m3`)

Confirmed on the ZPOD (Pi Zero W):

- **Boot time:** 1 min 28 s (M2 took 3 min 11 s to multi-user).
- **Battery:** the battery display works and persists after a reboot (`i2c-dev` fix).
- **Shutdown screen:** Shutdown shows "Safe to power off".

Not reported yet: automatic USB fix on macOS, new button actions, 0.6 s volume repeat.

## 0. Before first boot (on the Mac) — optional `userconf` helper

The data USB port is a gadget (no OTG keyboard for the HDMI wizard), so create
the user headlessly. After writing the image, re-mount the card and run
(prompts for the password; nothing is stored in this repo):

```bash
echo "paul:$(openssl passwd -6)" > /Volumes/bootfs/userconf.txt
```

**Wi-Fi (`v0.4.1-m3`+; the image has no Wi-Fi credentials):** add yours on the
boot volume. It is imported on first boot and the file is deleted
([BOOT-WIFI.md](./BOOT-WIFI.md)):

```bash
printf 'SSID=%s\nPSK=%s\n' 'YourSSID' 'YourPassword' > /Volumes/bootfs/zpod-wifi.txt
```

Skip this to use only the USB cable. USB networking does not need Wi-Fi.
Eject, insert, power on. After boot: `journalctl -b -u zpod-wifi-import` shows
"imported", `ls /boot/firmware/zpod-wifi.txt` → no such file, and
`sudo ls -l /etc/NetworkManager/system-connections/` shows `zpod-wifi.nmconnection`
with mode `-rw-------`.

## 1. USB link (M2 bug: Mac got only 169.254.x.x)

| # | Check | Pass |
| --- | --- | --- |
| 1.1 | Plug the data port into the Mac, boot | within ~2 min the Mac's "Raspberry Pi USB Gadget" interface gets `10.12.194.x` (`ifconfig en14`) **without** any manual step |
| 1.2 | `ssh paul@10.12.194.1` | works with the Mac's Wi-Fi off |
| 1.3 | `journalctl -b -u zpod-usb-kick` | either `usb0 ok (rx=N)` or `re-enumerating … ` followed by a working link |

## 2. Battery (M2 bug: no `/dev/i2c-1`)

| # | Check | Pass |
| --- | --- | --- |
| 2.1 | `ls /dev/i2c-1` | exists right after boot (`i2c-dev` loaded via `modules-load.d/zpod.conf`) |
| 2.2 | Screen header / Status | battery % shown (not `n/a`); `sudo i2cget -y 1 0x36 0x04 w` plausible |

## 3. Buttons (vendor labels, [BUTTONS.md](./BUTTONS.md))

| # | Check | Pass |
| --- | --- | --- |
| 3.1 | Up / Down | move the menu selection |
| 3.2 | Circle or D-pad centre | OK / select; also "yes" in Reboot / Shutdown |
| 3.3 | X or Left | cancel / back; "no" in confirmations |
| 3.4 | Home or Square | back to the main menu |
| 3.5 | Triangle | full-screen clock on; press again (or Home) → off |
| 3.6 | Play/Pause | starts the 440 Hz tone; press again → stops |
| 3.7 | Side volume, short press (~0.5 s) | exactly one 3 % step (M2: a 0.64 s press gave several); hold > 0.6 s → repeats |

## 4. Shutdown screen (M2 bug: frozen "Shutting down" frame)

Default `/etc/zpod/ui.conf` → `[shutdown] screen = message`.

| # | Check | Pass |
| --- | --- | --- |
| 4.1 | Menu → Shutdown → OK | "Shutting down", then a black screen with green frame: **"ZPOD off / Safe to power off"**, still legible after the Pi halts (ACT LED off) |
| 4.2 | `sudo reboot` (SSH) | "Rebooting..." frame, then the normal UI after boot |
| 4.3 | `sudo systemctl stop zpod-ui` | screen goes black (backlight off if GPIO27 controls it); `start` brings it back |
| 4.4 | Optional: set `screen = off`, `sudo poweroff` | black screen, backlight off — **only use if 4.3 showed GPIO27 really switches the backlight** (runtime test 2026-10-17 17:28:56–17:29:46 PT) |

## 5. Boot time (M2 on hardware: multi-user after 3 min 11 s)

| # | Check | Pass |
| --- | --- | --- |
| 5.1 | `systemd-analyze` (2nd boot or later) | userspace clearly shorter than M2's 3 min 14 s |
| 5.2 | `systemd-analyze blame \| head` | no `console-setup`, `apt-daily*`, `udisks2`, `NetworkManager-wait-online` |
| 5.3 | `systemctl is-enabled apt-daily.timer apt-daily-upgrade.timer console-setup` | `masked` ×3 |
| 5.4 | `rfkill list` | Wi-Fi unblocked, Bluetooth soft-blocked (unchanged; `systemd-rfkill` kept) |

## 6. Regression (from TEST-M2)

Audio (tone audible, red LED by the volume buttons lights during playback =
DAC GPIO4 amp-enable), `aplay -l` shows `sndrpiaoidezpod`, Wi-Fi power save
off, persistent journal, `get_throttled` = `0x0`.

Useful logs: `journalctl -b -u zpod-ui -u zpod-buttons -u zpod-usb-kick`,
`systemd-analyze blame`, `dmesg | grep -i -E 'dwc2|g_ether|aoide'`.
