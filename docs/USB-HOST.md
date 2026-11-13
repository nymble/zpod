# USB host mode: keyboard, mouse and USB GPS on the data port

The Pi Zero W has one USB controller (dwc2) on its micro-USB **data** port.
It is either a **gadget** (the default: USB Ethernet to the computer,
10.12.194.1, file transfer and SSH over the cable) or a **host** (keyboard,
mouse, USB serial GPS through an OTG adapter or hub). Not both at once.
The boot role is set on the SD card's FAT boot partition, and it can now
also be **switched live from the screen** (Tools > USB, or the prompt in
GPS / Mouse Music), no SD card and no reboot. Plan:
[USB-HOST-PLAN.md](./USB-HOST-PLAN.md).

In host mode the USB link to the computer is gone: use Wi-Fi for SSH and
the dev loop ([DEV-LOOP.md](./DEV-LOOP.md), `scripts/zpod-dev --save-host`).

## Switch from the screen (no SD card)

**Tools > USB** shows the role now (*Computer link* / *USB devices* /
*Auto*), the computer link or the plugged-in devices, whether Wi-Fi SSH
works (`root@<Wi-Fi IP>`), and the role the next boot uses. Three rows:

| Row | Does |
|---|---|
| Use USB for devices | host: keyboard, mouse, GPS through an OTG adapter or hub |
| Connect to computer | gadget: USB Ethernet 10.12.194.1, SSH, file transfer |
| Auto: follow the cable | otg: the cable's ID pin decides (experimental, not yet tried with an OTG adapter) |

OK on a row opens *Switch USB to devices?* / *Connect to computer?*:

- **OK** switches now (about 1 s to host, 4 s back to the computer).
- **Triangle** (only shown without Wi-Fi SSH): switch to devices anyway.
  Without Wi-Fi SSH the ZPOD can then only be switched back from its own
  screen (or by restarting it).
- **Square** (images with the boot guard, after test3): switch and keep it
  for the next boot too.
- **X / Left** cancels.

The apps that need host mode ask themselves: **GPS** opened while USB is
the computer link shows *Switch USB to devices?* (OK = switch, then it
looks for the receiver; Square in GPS = back to the computer). **Mouse
Music**: choosing *USB mouse + keyboard* on the chooser asks the same;
Square on the chooser gives the port back. Apps need
`permissions = ["usb-host"]` in their manifest, and only the app on the
screen can ask (the launcher checks the caller's pid).

Over SSH (Wi-Fi): `zpod-app usb` (status), `zpod-app usb host`,
`zpod-app usb gadget`; or the tool itself, `zpod-usb-role`,
`zpod-usb-role check`, `zpod-usb-role set host|gadget|auto [--force]`.
`set host` refuses (exit 3) unless the Wi-Fi has an address and sshd is
listening; `--force` overrides.

### How the live switch works (verified on test3, kernel 6.12.61)

dwc2 reads `dr_mode` from the device tree only when it probes
(`dwc2_get_dr_mode()` in `drivers/usb/dwc2/platform.c`); the
debugfs `dr_mode` file is read-only and the `usb-role-switch` class path
needs `dr_mode=otg` plus a `usb-role-switch` DT property, which the Pi's
dwc2 node does not have. So `zpod-usb-role set` does: stop the gadget,
`echo 20980000.usb > /sys/bus/platform/drivers/dwc2/unbind`, write a
one-property overlay (`/usr/lib/zpod/usb-role/<mode>.dtbo`, `dr_mode` on
`/soc/usb@7e980000`) to `/sys/kernel/config/device-tree/overlays/zpod-usb-role/dtbo`
(`CONFIG_OF_OVERLAY` + `CONFIG_OF_CONFIGFS`), bind dwc2 again, start the
gadget if it is one. On test3 the host root hub appeared in about 0.6 s;
back to gadget (remove the overlay, bind, `S35usbgadget start`) the Mac
link was up again in about 4 s; three host/gadget cycles in a row worked.
The kernel logs `OF: overlay: memory leak will occur if overlay removed`
once per removal (a few hundred bytes, harmless). With `otg` both the host
controller and the gadget come up and the Mac link works with no OTG
adapter; the ID-pin change with an adapter is not verified yet.
Sources: Raspberry Pi forum threads 282649 and 383596,
tsengf.blogspot.com 2025/02 "PiZero OTG host or peripheral", and the
kernel source above.

The overlay lives in RAM: **a reboot or power cycle always returns to the
boot role** (gadget unless the boot partition says host). The switch rolls
itself back to the boot role if the new role does not come up within 5 s.
Logs: `/run/zpod/usb.log` (never Wi-Fi names or passwords).

### Recovery

1. On the screen: Tools > USB > *Connect to computer* > OK.
2. Restart the ZPOD (Power > Restart, or power off/on): a live switch is
   gone after any reboot.
3. Over Wi-Fi: `ssh root@<Wi-Fi IP> zpod-app usb gadget`.
4. Images after test3, when the boot partition was set to host (Square /
   `--persist` / `zpod-usb-role next-boot host`):
   - **hold Home while powering on**: the firmware's `[gpio24=0]` filter
     in `config.txt` makes that boot a gadget (VERIFY-ON-DEVICE: the
     filter relies on the GPIO24 pull-up from the `gpio=` line being set
     before the filter is read; if it is not, the filter is always true,
     which fails safe as gadget);
   - **boot guard**: `S35usbgadget` runs `zpod-usb-role boot-guard` on a
     host-mode boot; if the launcher is not up 120 s later, or after the
     third host boot in a row that never reached it, it writes gadget back
     to the boot partition and switches to gadget at once;
   - on the Mac: the SD card edits below.

`next-boot` keeps the boot partition consistent: on test4 it toggles
`#dtparam=dr_mode=host` in `zpod-usb-role.txt`; on test3 (no such file) it
changes the one `dtoverlay=dwc2,dr_mode=...` line in `config.txt` and keeps
`config.txt.zpod-prev`. It remounts the partition rw only if it was ro,
writes a temporary file, syncs, renames, syncs, restores ro and reads the
result back. test3's S35 has no boot guard, so the screen does not offer
*keep for next boot* there.

### test3 with the dev core overlay

test3's read-only root has no `zpod-usb-role`; `scripts/zpod-dev --core`
installs it to `/data/zpod/core/zpod-usb-role` (+ `usb-role/*.dtbo`,
root-owned; the launcher only runs it when root owns it and its directory
and neither is group/other-writable). As for any `--core` change on test3,
run it again after each reboot. On test3 `/dev/ttyUSB0` is `root:root 660`,
so the GPS app (user `zpod`) cannot open it after a switch until
`chmod 666 /dev/ttyUSB0` over SSH (lost on unplug); fixed in the next
image (group `dialout`).

## Switch on the SD card

### Before switching (any image)

1. The ZPOD is on your Wi-Fi as a client (Tools > Wi-Fi shows *Client* and
   an address), and SSH over Wi-Fi works from the Mac:

   ```zsh
   cd ~/cb/projects/zpod
   scripts/zpod-dev --save-host
   ```

   ```zsh
   scripts/zpod-dev --which-host
   ```

   The second command must print the Wi-Fi address (not 10.12.194.1).
2. Shut the ZPOD down (Power > Shut down), take the card out, put it in the
   Mac. The boot partition mounts as `/Volumes/ZPODBOOT` (Buildroot images).

### Images after test3 (zpod-usb-role.txt)

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

### test3 card (v0.9.0-br-m6-fastboot-test3)

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
