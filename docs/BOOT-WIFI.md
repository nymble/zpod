# First-boot Wi-Fi (M1.1+)

The M1.1 image ships **SSH + Wi-Fi stack ready** for Pi Zero W / Zero 2 W
(`wpasupplicant`, `network-manager`, `firmware-brcm80211`, `ENABLE_SSH=1`,
regulatory domain `US`).

If the builder has a local gitignored `config/wifi.local.env`, the image may
also ship a preconfigured home SSID (NetworkManager + boot `wpa_supplicant.conf`).
That file is never committed; GitHub issue comments only say "home SSID configured".
Without `wifi.local.env`, set Wi-Fi yourself using the steps below.

Hostname: `zpod`. User: `pi` (set password on first boot via HDMI wizard, or
via Raspberry Pi Imager OS customization before first boot).

## Recommended: Raspberry Pi Imager OS customization (Mac)

1. Download the release `.img.xz` and verify `SHA256SUMS`.
2. Open **Raspberry Pi Imager** → choose OS → **Use custom** → select the
   decompressed `.img` (or the `.xz` if your Imager accepts it).
3. Choose your SD card (double-check the target).
4. Click the gear / **Edit settings** (OS customization):
   - Set hostname `zpod` (optional; already baked in).
   - Enable SSH (already enabled in the image; safe to leave on).
   - Set username/password for `pi` (skips HDMI password wizard if Imager
     writes it).
   - **Configure wireless LAN**: enter your SSID, password, and country `US`
     (or your country).
5. Write the card, insert into the Zero W / Zero 2 W, power on.
6. From the Mac (same Wi-Fi), wait ~1–2 minutes, then:

```bash
ssh pi@zpod.local
# or: ssh pi@<ip-from-router>
```

No HDMI required once Imager has set Wi-Fi + password.

## Alternative: `wpa_supplicant.conf` on the boot partition

If you prefer not to use Imager customization (or write the raw image with
`dd` / Balena Etcher):

1. Write the image to the SD card.
2. Remount the card on the Mac. The small **boot** volume should appear
   (FAT; often named `bootfs` or similar).
3. Create a file on that volume named `wpa_supplicant.conf` with:

```
ctrl_interface=DIR=/var/run/wpa_supplicant GROUP=netdev
update_config=1
country=US

network={
    ssid="YOUR_SSID"
    psk="YOUR_PASSWORD"
    key_mgmt=WPA-PSK
}
```

4. Also create an empty file named `ssh` on the boot volume if you want to
   force-enable SSH (already enabled in M1.1, but harmless).
5. Eject, boot the Pi. On first boot, Raspberry Pi OS net mods / first-run
   paths consume the file where supported; if association fails, use HDMI
   once and run `sudo raspi-config` → System Options → Wireless LAN, or:

```bash
sudo nmcli device wifi connect 'YOUR_SSID' password 'YOUR_PASSWORD'
```

6. SSH from the Mac: `ssh pi@zpod.local`

## After you are on the network

```bash
hostname
systemctl is-active ssh NetworkManager
iwconfig 2>/dev/null || nmcli device status
i2cdetect -l
```

## Pi Zero (no radio)

The same image family must still boot a Pi Zero with no Wi-Fi. Boot does not
wait for wireless. Skip the Wi-Fi steps; use HDMI + keyboard.

## What we deliberately do not ship

- Your SSID or Wi-Fi password (unknown to the builder; never invent them).
- TFT / DAC / button device-tree overlays.
- A baked default password for `pi`.
