# Labs roadmap: USB, BLE, Wi-Fi (and drone Remote ID)

Professional test tools on the ZPOD, one thread per radio, built in Rust and
MIT-licensed. **R1 = test7** is the receive-and-inspect foundation. Nothing in
R1 (test7) transmits: no HID gadget, no BLE advertising, no over-the-air Remote ID,
no packet injection. The one generator in R1 is **offline**: it writes a
labelled TEST aircraft into a pcap file on a computer (see below).

## R1 (test7) at a glance

| Tool | Where | What it does | Needs |
|---|---|---|---|
| USB Lab (`usb-lab`) | Tools | Lists USB devices (port in host mode), reads every descriptor from sysfs, decodes HID report maps, flags keystroke-injector shapes (a keyboard combined with storage, network or serial in one device; a keyboard from an unknown vendor ID or with no product name; Linux gadget IDs). Saves `usb-<time>.txt` + `.json` reports. | USB host mode (it asks), an OTG adapter or powered hub |
| BLE Lab (`ble-lab`) | Tools | Passive LE scan on the onboard radio: device table (signal, count, seen), classes (Apple Continuity incl. AirPods models, Find My, iBeacon, Eddystone, Fast Pair, Swift Pair, Tile, Samsung, company IDs), drone Remote ID over Bluetooth (ASTM F3411). JSONL capture, text reports. | Nothing extra |
| Wi-Fi Lab (`wifi-lab`) | Tools/Radio | Monitor mode on a **USB** adapter only, channel set / discovery sweep 1-6-11 (+44, 149 on a dual-band adapter), beacon and NAN decode of drone Remote ID, concise live aircraft list (id, position, height, transport, age, signal) with a detail page, pcap recording. | Monitor-capable USB adapter (RT5370 / RT5372 / RT3070 / MT7601U) on a powered hub |

Screens (simulator): [USB list](screenshots/test7-usblab-list.png),
[USB detail](screenshots/test7-usblab-combo-detail.png),
[BLE list](screenshots/test7-blelab-list.png),
[BLE drone](screenshots/test7-blelab-detail-drone.png),
[Wi-Fi Remote ID](screenshots/test7-wifilab-listening.png),
[Wi-Fi aircraft](screenshots/test7-wifilab-detail.png),
[no adapter](screenshots/test7-wifilab-no-adapter.png),
[TEST track list](screenshots/test7-wifilab-test-list.png),
[TEST track detail](screenshots/test7-wifilab-test-detail.png).

## Architecture

```
 usb-lab ──── sysfs (/sys/bus/usb/devices: descriptors, report_descriptor)   no root
 ble-lab ──┐
 wifi-lab ─┴─ /run/zpod-labd.sock ── zpod-labd (root, S50zpod-labd)
                                       ├ wlan-monitor IFACE CHAN / wlan-chan / wlan-managed
                                       ├ wlan-stream IFACE   (AF_PACKET, receive only)
                                       └ hci-stream          (HCI monitor + passive LE scan)
```

Crates (all `apps/crates`, MIT):

| Crate | Role |
|---|---|
| `zpod-remoteid` | ASTM F3411 decoder (Basic ID, Location, Self ID, System, Operator ID, message pack), transports (BLE 4 / 5 service data UUID 0xFFFA, Wi-Fi beacon vendor IE OUI FA:0B:BC type 0x0D, NAN SDF service ID 88 69 19 9D 92 09), aircraft tracker, display lines. `encode` + `sim`: offline encoder for a labelled TEST aircraft and a moving-track model (bytes in memory only). |
| `zpod-usb` | sysfs scan, raw descriptor parser, HID report-descriptor summary, injector indicators, known-device table (+ user file `/data/zpod/appdata/usb-lab/known.txt`), text/JSON reports |
| `zpod-ble` | HCI monitor / LE (extended) advertising report parser, AD parser, classifier, device table, JSONL |
| `zpod-wlan` | adapter inventory (onboard vs USB, `iw phy` caps), radiotap, 802.11 frame + IE parser, Remote ID frame decode, pcap writer/reader, `testgen` (offline TEST-track pcap) |
| `zpod-labd` | root helper: line protocol, validation, Linux server, client |
| `zpod-labui` | shared lab screens (header, list, detail, message, ask) |

### zpod-labd rules (the receive-only guarantee)

- Socket `/run/zpod-labd.sock`, one command per connection; `SO_PEERCRED`
  allows root and the `zpod` user only.
- **Refuses the onboard radio** (`brcmfmac` / SDIO): monitor mode is only
  ever set on a USB adapter. The onboard Wi-Fi stays on your network.
- Streams are copied out frame by frame (`[u32 LE len][bytes]`, 0 = keep-alive).
  **No socket or file descriptor is ever handed to an app**, so an app cannot
  inject.
- BLE uses a **passive** scan (HCI LE Set Scan Parameters with type 0, then
  Enable): no scan requests are sent. `btmgmt power on` is the only state
  change. Ending the stream disables the scan.
- Leaving a Wi-Fi stream (or the app) puts the adapter back in managed mode.

Deliberate choices vs the research notes (lower complexity, no extra
libraries in the image): sysfs instead of `nusb` / `hidreport` (no root, no
device access); the HCI monitor channel instead of `bluer` / D-Bus (no libdbus
cross-build, and BlueZ discovery would send scan requests); AF_PACKET + an own
pcap writer instead of libpcap; an own decoder crate instead of the
`remoteid` crate (receive-only, no encoder in the image).

## Offline TEST aircraft (R1, encode only)

For decoder, Wireshark and track-to-map tests without any radio:

```sh
cd apps && cargo run -p zpod-wlan --example rid-test-pcap -- /tmp/test-track.pcap \
    37.7749 -122.4194 12 45 1 2 90 6
#   lat lon speed_mps track_deg climb_mps turn_dps seconds channel
```

- One beacon per second (radiotap + 802.11 beacon, SSID `ZPOD-TEST-RID`,
  locally administered MAC `02:5a:50:00:7e:57`) with a Remote ID message
  pack: Basic ID `ZPOD-TEST-0001`, Location, Self ID `ZPOD TEST - NOT A
  UA`, System (operator = start point), Operator ID.
- `SimAircraft` models position, ground speed, track (direction),
  altitude/height and climb rate, with a constant turn rate, so the aircraft
  moves smoothly and a map under test can draw its track.
- Replay on the device or in the simulator: `ZPOD_WLAN_REPLAY=<pcap>
  wifi-lab`. Tests: `zpod-remoteid` (encode/decode round trip, sim motion)
  and `zpod-wlan` (30-frame track pcap decodes as one moving aircraft).
- Not in R1: sending these frames. The image has no transmit path
  (`zpod-labd` has no inject command; apps never get a raw socket).

## test8 (v0.13.0-br-ab-test8)

Ships gated **Remote ID TEST transmit** (Wi-Fi Lab: Square → safeguard →
confirm → arm) and **HID Lab** (DuckyScript library, select/edit/dry-run,
confirm+arm; Tools > USB > USB HID keyboard). Every transmit still requires
the per-run warning. Receive paths unchanged from test7.

## Paul's lab decisions (2026-11-20)

- **Dongles** (Wi-Fi, BLE) can go on Pi Lab or the ZPOD, or both, for
  development. When a dongle takes the ZPOD's only USB port, the ZPOD stays
  reachable over its onboard Wi-Fi (SSH), or use a powered USB hub. Wi-Fi
  Lab's empty state says this.
- **Lab environment:** isolated lab with cables, dummy-load antennas and
  Faraday enclosures. Transmit features are allowed there behind a
  **safeguard warning before every transmit** (not once per session):
  "isolated lab: cabled / dummy load / Faraday enclosure in place?", with an
  explicit confirm and arm, default off, never at boot.
- **Drone Remote ID transmit (later, gated):** the simulator models
  location, speed, altitude and direction over time (R1 `SimAircraft`) so
  receivers and track-to-map tools can be tested with a moving aircraft;
  several aircraft and scripted routes (waypoints) next. Every generated
  aircraft is labelled TEST.
- **Reception first:** band scanning and discovery, and a concise live
  display (one row per aircraft: id, position, height, transport, age,
  signal; detail on OK) that updates as frames arrive.
- **DuckyScript / USB HID (later, gated, off in R1):** collect community
  DuckyScript payloads into a library on `/data`, browse and **select** one,
  **edit** it on the device (USB keyboard, or the on-screen character grid
  from the Wi-Fi password entry), dry-run (show the keystrokes), then run
  only after the confirm-and-arm prompt with the target connected. R1 ships
  no HID gadget.
- **AirPods-style test advert (later, gated):** a Continuity-format test
  advertisement for testing scanners in the isolated lab. It cannot pair or
  play audio.

## Hardware

- Pi Zero W: one micro-USB data port. Labs that need USB devices switch it to
  host mode (with the usual Wi-Fi SSH safety prompt) and give it back with
  Triangle.
- Wi-Fi adapters: use a **powered hub**; the Zero W cannot feed an adapter's
  transmit peaks reliably. Firmware in test7: `rt2870.bin` (rt2800usb: RT5370,
  RT5372, RT3070, RT2870) and `mediatek/mt7601u.bin`. Kernel modules were
  already in the image.

## Later releases (plan, not in test7)

Each step that transmits needs Paul's explicit go, an off-by-default switch,
an on-screen warning, and a lab environment where transmitting is legal.

| Release | USB | BLE | Wi-Fi / drones |
|---|---|---|---|
| R2 | live HID event view (decoded keystrokes from a plugged device), descriptor diff vs known, export to the Mac | RSSI trend + "following me" detector (same random address family over time), AirPods / Find My battery & state decode | channel survey (per-channel frame rate, busy), probe-request view, Remote ID on 5 GHz with a dual-band adapter |
| R3 | HID gadget **emulation** for authorised testing (Bash Bunny style): DuckyScript library (community scripts) with select + on-device edit + dry run, confirm-and-arm before every run, never at boot | GATT browser (connect, read only) | pcap export to the Mac over USB/Wi-Fi, Wireshark-ready; multi-aircraft / waypoint TEST pcaps |
| R4 | USB protocol fuzzing (gadget side), for own devices only | test advertisers (beacon, Remote ID, AirPods-style Continuity) behind the per-transmit safeguard warning | Remote ID **test transmitter** (moving TEST aircraft from `SimAircraft`) behind the per-transmit safeguard warning, isolated lab only |

Not planned: impersonating a real device to a third party outside the lab,
deauthentication, jamming, or anything that disturbs other people's
networks.
