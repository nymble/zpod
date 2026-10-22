# USB cable networking to a Mac (M2+)

The Pi Zero's **data** micro-USB port (the one marked `USB`, not `PWR`) is a
USB Ethernet gadget. A single data-capable USB cable to the Mac powers the Pi
**and** gives a network link that does not depend on Wi-Fi.

## Why M4 changed the gadget (root cause of "USB networking does not work")

M2/M3 used the `g_ether` module, which offers the host two USB
configurations (RNDIS and CDC-ECM). macOS picks CDC-ECM and binds its
DriverKit **AppleUserECM** driver. That driver **stops transmitting after
about 161 frames per enumeration** (Apple FB24614121; measured by several
users in [raspberrypi/rpi-usb-gadget#27](https://github.com/raspberrypi/rpi-usb-gadget/issues/27):
host `Opkts` freezes at 161, the Pi's `usb0 rx_packets` stays at 1). The
Pi's frames still reach the Mac, so the interface looks "up", but the Mac's
DHCP requests never reach the Pi: no lease, self-assigned 169.254.x.x. This
is what we saw on M2 (rx_packets stuck at 1). M3's `zpod-usb-kick`
re-enumerates the gadget, which only resets the 161-frame budget, so it
"sometimes worked for a moment". Nothing on the Mac side can fix it without
disabling SIP.

**M4 fix:** a configfs (libcomposite) gadget with **one** function,
**CDC-NCM**. macOS drives NCM with a different built-in driver, Linux and
Windows 11 support NCM natively, and there is still nothing to install or
configure on the Mac. `/boot/firmware/zpod-usb.txt` selects the mode:
`mode=ncm` (default), `mode=ecm` (CDC-ECM only), `mode=g_ether` (the M3
module, for Windows 10 / debugging). Edit it from the Mac with the card in
a reader.

Known risk: on a **Pi 5** the NCM gadget can lock up during link-up
([raspberrypi/linux#7622](https://github.com/raspberrypi/linux/issues/7622)).
The ZPOD is a Pi Zero W (dwc2, different controller); hardware test pending.

## What the image configures

| Item | Value |
| --- | --- |
| `config.txt` | `dtoverlay=dwc2,dr_mode=peripheral` |
| Gadget | `zpod-usb-gadget.service` (sysinit, before NetworkManager) runs `/usr/lib/zpod/zpod-usb-gadget.sh`: configfs gadget `zpod`, VID:PID 1d6b:0104, serial `zpod`, product "ZPOD USB Ethernet", one `ncm.usb0` function. `g_ether` is no longer loaded |
| Fixed MACs | Mac side `02:5a:50:4f:44:01`, Pi side `02:5a:50:4f:44:02`, so macOS sees the same interface each boot |
| NM profile `USB Gadget (shared)` | autoconnect; Pi = **10.12.194.1/28**, Pi runs DHCP for the Mac (10.12.194.2–14); IPv6 link-local |
| NM profile `USB Gadget (client)` | used automatically by `rpi-usb-gadget-ics.service` only if the Mac turns on Internet Sharing for that interface |
| DHCP options | **no default route / DNS** handed to the Mac (`91-zpod-usb-no-default-route.conf`), so the Mac keeps using its own internet |
| usb0 managed | udev + NM override (NM unmanages gadget devices by default) |
| mDNS | `avahi-daemon` answers `zpod.local` on usb0 as well as wlan0 |
| `zpod-usb-kick` | kept as a safety net: re-enumerates once if usb0 has carrier but received ≤ 2 frames after 25 s |

## Diagnose from the Mac (no SSH needed)

```bash
system_profiler SPUSBDataType | grep -A8 -i zpod     # is the Pi enumerated at all?
ioreg -p IOUSB -l -w0 | grep -i -E 'zpod|gadget'
networksetup -listallhardwareports | grep -A2 -i -E 'zpod|gadget'
ifconfig | grep -B3 -A6 '02:5a:50:4f:44:01'          # the Mac side of the cable
netstat -I en7 -b                                    # replace en7; Opkts frozen near 161 = the ECM bug
```

No ZPOD in the USB tree = charge-only cable, wrong port (`PWR`), or the Pi
has not booted. An interface with 169.254.x.x and Opkts stuck near 161 = the
ECM bug (M3 image); M4 uses NCM.

## Use it from a Mac

1. Plug a **data** USB cable from the Mac into the Pi's `USB` port (the
   `PWR` port can stay empty — the Mac powers the Pi).
2. Wait ~60–90 s for boot. In **System Settings → Network** a new interface
   appears (M4: *ZPOD USB Ethernet*; M3: *RNDIS/Ethernet Gadget*).
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
systemctl status zpod-usb-gadget   # "ncm on 20980000.usb, ifname usb0"
ls /sys/kernel/config/usb_gadget/zpod/functions   # ncm.usb0
nmcli device status            # usb0 connected "USB Gadget (shared)"
ip -4 addr show usb0
```

## Troubleshooting

- **No interface on the Mac:** cable is charge-only, or plugged into `PWR`.
  Try another cable. On the Pi: `journalctl -b -u zpod-usb-gadget`,
  `dmesg | grep -i -E 'dwc2|gadget|ncm'`.
- **Interface but self-assigned 169.254.x.x (seen on M2/M3 + macOS; root cause above):**
  usb0 is up and the Mac receives the Pi's frames, but the Pi receives none
  (`cat /sys/class/net/usb0/statistics/rx_packets` stays at 1). A clean
  re-enumeration resets it for ~161 frames; M4 avoids it with NCM. By
  hand: `echo disconnect | sudo tee /sys/class/udc/*/soft_connect; sleep 2;
  echo connect | sudo tee /sys/class/udc/*/soft_connect`.
- **Interface but self-assigned 169.254.x.x (other cause):** NetworkManager did not bring
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
