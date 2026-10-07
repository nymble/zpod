# USB cable networking to a Mac (M2+)

The Pi Zero's **data** micro-USB port (the one marked `USB`, not `PWR`) is a
USB Ethernet gadget. A single data-capable USB cable to the Mac powers the Pi
**and** gives a network link that does not depend on Wi-Fi.

## What the image configures

Raspberry Pi's own **`rpi-usb-gadget`** package (trixie) is installed, and the
image is pre-set exactly as `sudo rpi-usb-gadget on` would leave it, plus a
few ZPOD tweaks:

| Item | Value |
| --- | --- |
| `config.txt` | `dtoverlay=dwc2,dr_mode=peripheral` |
| Kernel module | `g_ether` (`/etc/modules-load.d/usb-gadget.conf`), CDC-ECM + RNDIS |
| Fixed MACs | Mac side `02:5a:50:4f:44:01`, Pi side `02:5a:50:4f:44:02` (`/etc/modprobe.d/zpod-g_ether.conf`), so macOS sees the same interface each boot |
| NM profile `USB Gadget (shared)` | autoconnect; Pi = **10.12.194.1/28**, Pi runs DHCP for the Mac (10.12.194.2–14); IPv6 link-local |
| NM profile `USB Gadget (client)` | used automatically by `rpi-usb-gadget-ics.service` only if the Mac turns on Internet Sharing for that interface |
| DHCP options | **no default route / DNS** handed to the Mac (`91-zpod-usb-no-default-route.conf`), so the Mac keeps using its own internet |
| usb0 managed | udev + NM override (NM unmanages gadget devices by default) |
| mDNS | `avahi-daemon` answers `zpod.local` on usb0 as well as wlan0 |

## Use it from a Mac

1. Plug a **data** USB cable from the Mac into the Pi's `USB` port (the
   `PWR` port can stay empty — the Mac powers the Pi).
2. Wait ~60–90 s for boot. In **System Settings → Network** a new interface
   appears (named like *RNDIS/Ethernet Gadget* or *Raspberry Pi USB Gadget*).
   It should get a `10.12.194.x` address automatically. The first time,
   macOS may ask to allow the accessory — click **Allow**.
3. Connect:

   ```bash
   ssh <user>@zpod.local          # mDNS over the cable (or Wi-Fi)
   ssh <user>@10.12.194.1         # always the Pi over the cable
   ```

4. Copy files: `scp file.mp3 <user>@10.12.194.1:~/` or `rsync -av dir/ <user>@10.12.194.1:~/dir/`.

If `zpod.local` resolves to the Wi-Fi address instead, that is fine (both
work); use `10.12.194.1` to force the cable path.

## Check on the Pi

```bash
rpi-usb-gadget status          # "USB Gadget mode is on", usb0 IPv4 10.12.194.1/28
nmcli device status            # usb0 connected "USB Gadget (shared)"
ip -4 addr show usb0
```

## Troubleshooting

- **No interface on the Mac:** cable is charge-only, or plugged into `PWR`.
  Try another cable. `dmesg | grep -i -E 'dwc2|gadget|g_ether'` on the Pi.
- **Interface but self-assigned 169.254.x.x:** NetworkManager did not bring
  up the shared profile: `sudo nmcli con up "USB Gadget (shared)"`, and
  check `nmcli device` shows usb0 as managed.
- **Want the Pi to use the Mac's internet:** enable *Internet Sharing* on the
  Mac for the gadget interface; `rpi-usb-gadget-ics` switches the Pi to the
  client profile automatically.

## First boot / creating the user (important)

`dr_mode=peripheral` means the data port is **not** a USB host, so a USB
keyboard on an OTG adapter does **not** work for the HDMI first-boot
password wizard. Create the user headlessly instead, before first boot:

```bash
# On the Mac, with the freshly written card mounted (volume "bootfs"):
echo "paul:$(openssl passwd -6)" > /Volumes/bootfs/userconf.txt
```

(`userconf-pi` renames the default `pi` account to that name with that
password on first boot.) Raspberry Pi Imager OS customisation also works.

If you really want the HDMI + OTG-keyboard wizard, edit `config.txt` on the
card and change the line to `dtoverlay=dwc2,dr_mode=otg` (or comment it out)
for the first boot, then restore it.
