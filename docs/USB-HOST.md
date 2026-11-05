# USB host mode: keyboard, mouse and USB GPS on the data port

The Pi Zero W has one USB controller (dwc2) on its micro-USB **data** port.
It is either a **gadget** (the default: USB Ethernet to the computer,
10.12.194.1, file transfer and SSH over the cable) or a **host** (keyboard,
mouse, USB serial GPS through an OTG adapter or hub). Not both at once.
The role is chosen at boot on the SD card's FAT boot partition, so it can
always be put back on the Mac. Plan: [USB-HOST-PLAN.md](./USB-HOST-PLAN.md).

In host mode the USB link to the computer is gone: use Wi-Fi for SSH and
the dev loop ([DEV-LOOP.md](./DEV-LOOP.md), `scripts/zpod-dev --save-host`).

## Before switching (any image)

1. The ZPOD is on your Wi-Fi as a client (Tools > Wi-Fi shows *Client* and
   an address), and SSH over Wi-Fi works from the Mac:

   ```zsh
   cd ~/src/zpod
   scripts/zpod-dev --save-host
   ```

   ```zsh
   scripts/zpod-dev --which-host
   ```

   The second command must print the Wi-Fi address (not 10.12.194.1).
2. Shut the ZPOD down (Power > Shut down), take the card out, put it in the
   Mac. The boot partition mounts as `/Volumes/ZPODBOOT` (Buildroot images).

## Images after test3 (zpod-usb-role.txt)

`config.txt` line 46 is the gadget overlay and line 51 includes
`zpod-usb-role.txt` (do not edit those). Host mode = one line in
`zpod-usb-role.txt`, line 13:

| | `/Volumes/ZPODBOOT/zpod-usb-role.txt` line 13 |
|---|---|
| gadget (default) | `#dtparam=dr_mode=host` |
| host | `dtparam=dr_mode=host` |

Switch to host (remove the `#`):

```zsh
sed -i '' 's/^#dtparam=dr_mode=host$/dtparam=dr_mode=host/' /Volumes/ZPODBOOT/zpod-usb-role.txt
```

```zsh
grep -n dr_mode /Volumes/ZPODBOOT/zpod-usb-role.txt
```

Back to gadget (put the `#` back; deleting the file also works):

```zsh
sed -i '' 's/^dtparam=dr_mode=host$/#dtparam=dr_mode=host/' /Volumes/ZPODBOOT/zpod-usb-role.txt
```

Then eject, put the card back and boot. On the device `zpod-usb-role`
prints the role, what the next boot will use, and the plugged-in devices:

```sh
ssh root@<Wi-Fi IP> zpod-usb-role
```

## test3 card (v0.9.0-br-m6-fastboot-test3)

test3 has no `zpod-usb-role.txt`. Host mode = edit the dwc2 line itself,
`/Volumes/ZPODBOOT/config.txt` **line 46**:

| | `/Volumes/ZPODBOOT/config.txt` line 46 |
|---|---|
| gadget (as flashed) | `dtoverlay=dwc2,dr_mode=peripheral` |
| host | `dtoverlay=dwc2,dr_mode=host` |

```zsh
sed -i '' 's/^dtoverlay=dwc2,dr_mode=peripheral$/dtoverlay=dwc2,dr_mode=host/' /Volumes/ZPODBOOT/config.txt
```

```zsh
grep -n dwc2 /Volumes/ZPODBOOT/config.txt
```

Revert:

```zsh
sed -i '' 's/^dtoverlay=dwc2,dr_mode=host$/dtoverlay=dwc2,dr_mode=peripheral/' /Volumes/ZPODBOOT/config.txt
```

Why this is enough on test3 (checked on the running test3 card, not yet
in host mode, so **VERIFY-ON-DEVICE**): the kernel has dwc2 with
`CONFIG_USB_DWC2_DUAL_ROLE=y`, and every host-side driver as a module
(`usbhid`, `hid-generic`, `usbserial`, `pl2303`, `cdc-acm`, `usb-storage`;
`evdev` is built in). `S35usbgadget` always loads `dwc2`; with
`dr_mode=host` the gadget script then fails harmlessly (no UDC) and `usb0`
never appears. mdev loads drivers by modalias when a device is plugged in.
`/dev/input/event*` is group `input`, which the app user `zpod` is in, so
keyboards and mice work in Mouse Music and on the Wi-Fi password page.
Wi-Fi (S40wifi) and SSH (S50dropbear, all interfaces) do not depend on USB.

Limits on test3, fixed in the next image:

- `/dev/ttyUSB*` is `root:root 660` on test3, so the GPS tool (runs as
  `zpod`) shows *No permission for /dev/ttyUSB0 (needs group dialout)*.
  Next image: `ttyUSB*`/`ttyACM*` are `root:dialout` and `zpod` is in
  `dialout`.
- `/run/zpod/usb-role` does not exist on test3; the apps read the role from
  the device tree instead (same answer).
- The GPS app's manifest uses `category = "tool"`, which test3's own
  `zpod-app` rejects: run `scripts/zpod-dev --core` first (after each
  reboot, as for any core change on test3).

## What works in host mode

| Device | Driver | Where |
|---|---|---|
| USB keyboard, mouse (HID boot / report) | usbhid + hid-generic -> evdev | Mouse Music (choose *USB mouse + keyboard*), Tools > Wi-Fi > network > password page |
| GlobalSat ND-105C (PL2303, 067b:2303) | usbserial + pl2303 -> /dev/ttyUSB0 | Tools > GPS (4800 8N1 NMEA) |
| CDC-ACM GPS / serial | cdc-acm -> /dev/ttyACM0 | Tools > GPS |
| Hub | usbcore | bus power from the Pi port is limited; a powered hub is safer (VERIFY-ON-DEVICE) |

Bluetooth input is shown as *later* in Mouse Music and not built.

## Security

Wi-Fi SSH is dropbear on port 22 on all interfaces (wlan0 included),
public-key only for root (`-s`: no password logins at all; `-g` is added
only when a userconf password exists). Anyone on the same LAN can reach
port 22 but needs an authorised key. The device's host key lives on
`/data`, so it stays the same across reboots (pin it; the scripts skip host
key checks only for the USB address 10.12.194.*). There is no mDNS on the
Buildroot image; the router's DNS may answer `zpod` (DHCP hostname).
