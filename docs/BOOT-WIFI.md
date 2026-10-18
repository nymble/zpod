# First-boot Wi-Fi

**ZPOD images never contain Wi-Fi credentials** (since `v0.4.1-m3`). You add
your network to the SD card's boot partition after flashing. On first boot the
ZPOD imports it into NetworkManager and deletes the file from the card.

> Images up to and including `v0.4.0-m3` were built with an embedded home Wi-Fi
> profile. They were withdrawn on 2026-10-18. Use `v0.4.1-m3` or later.

Without Wi-Fi the ZPOD is still reachable over the **USB cable**: plug the data
port into the Mac and `ssh <user>@10.12.194.1` (see [USB-GADGET.md](./USB-GADGET.md)).
Wi-Fi is optional.

## Mac: add Wi-Fi after flashing

1. Write the image to a disposable SD card (Raspberry Pi Imager "Use custom",
   or `dd`). Re-insert the card. The FAT boot volume mounts as `/Volumes/bootfs`.
2. Create your user. There is no keyboard for the HDMI wizard, because the USB
   port is in gadget mode. This command prompts for the password:

   ```bash
   echo "paul:$(openssl passwd -6)" > /Volumes/bootfs/userconf.txt
   ```

3. Add Wi-Fi. Replace the placeholders, and keep the single quotes:

   ```bash
   printf 'SSID=%s\nPSK=%s\n' 'YourSSID' 'YourPassword' > /Volumes/bootfs/zpod-wifi.txt
   ```

   To keep the password out of your shell history, prompt for it instead:

   ```bash
   read -r -s -p 'Wi-Fi password: ' PSK; echo
   printf 'SSID=%s\nPSK=%s\n' 'YourSSID' "$PSK" > /Volumes/bootfs/zpod-wifi.txt; unset PSK
   ```

   If you keep a local, gitignored `config/wifi.local.env` (`WPA_SSID=`,
   `WPA_PASSWORD=`, `WPA_COUNTRY=`) in your checkout,
   `scripts/write-zpod-wifi-txt.sh /Volumes/bootfs` writes the same file from
   it. The build never reads that file.

4. Eject the card (`diskutil eject /Volumes/bootfs`), put it in the ZPOD and
   power on.

### `zpod-wifi.txt` format

```
SSID=YourSSID
PSK=YourPassword
# optional:
COUNTRY=US
HIDDEN=yes
```

- One `KEY=value` per line, with no quotes around values. Everything after the
  first `=` is the value, spaces included. CRLF line endings and a UTF-8 BOM are OK.
- An empty or missing `PSK` means an open network. Otherwise PSK must be 8–63
  characters or 64 hex digits.
- `COUNTRY` (optional) sets the Wi-Fi regulatory domain. The image default is `US`.

You can also drop ready-made NetworkManager keyfiles (`*.nmconnection`) onto
the boot volume. They are installed as-is. Wi-Fi ones get `powersave=2` if
they don't set power save themselves.

## What happens on the ZPOD

`zpod-wifi-import.service` runs once per boot, after local filesystems are
mounted and **before NetworkManager starts**. It only runs if
`/boot/firmware/zpod-wifi.txt` or `/boot/firmware/*.nmconnection` exists.
Script: `/usr/lib/zpod/zpod-wifi-import.sh`.

1. It writes `/etc/NetworkManager/system-connections/zpod-wifi.nmconnection`
   (`root:root`, mode `0600`, `powersave=2` = Wi-Fi power save off, autoconnect).
   A later import replaces it.
2. It overwrites the source file with zeros, deletes it from the boot
   partition, and runs `sync`.
3. It never logs the SSID or the password. Check the result with
   `journalctl -b -u zpod-wifi-import`.

An invalid file (missing SSID, PSK too short) is still deleted, so no
credentials are left on the card. Fix the file and re-create it.

**Caveat:** SD cards remap blocks internally, so deleting a file from flash
does not guarantee the old bytes are gone from the chip. Treat a card that
held `zpod-wifi.txt` as holding the password.

## Change Wi-Fi later

- Over SSH: `sudo nmcli device wifi connect 'YourSSID' password 'YourPassword'`
- Or put a new `zpod-wifi.txt` on the boot volume and reboot.

## Build-side guarantees

- `build.sh` does not read `config/wifi.local.env` and writes no
  `WPA_SSID`/`WPA_PASSWORD` into the pi-gen config. Only `WPA_COUNTRY=US`
  is kept, so that WLAN is enabled.
- `scripts/export-image-offset.sh` refuses to export a rootfs that has a
  Wi-Fi NetworkManager profile, a PSK, or a `wpa_supplicant` config with a
  `psk`.
- `scripts/verify-m3-image.sh` fails if:
  - any `.nmconnection` other than the two usb0 profiles exists
  - any profile has Wi-Fi/PSK settings
  - a `wpa_supplicant` config has a `psk`
  - the boot FAT has `wpa_supplicant.conf`, `zpod-wifi.txt` or `*.nmconnection`
- `sudo scripts/check-image-secrets.sh <img>` searches the raw image, the
  root files and the boot files for your real SSID and password from the
  local env file. It prints match counts only; anything other than 0 fails.

## Pi Zero (no radio)

Boot does not wait for wireless. Skip the Wi-Fi steps and use the USB cable
or HDMI.
