# Test7 (R1): labs, synth, library fixes

Release `v0.12.0-br-ab-test7` (pre-release). Nothing here has run on
hardware yet: unit tests (host + ARMv6 under qemu-arm), the image's own
BusyBox under qemu-arm, QEMU `raspi0` boot and simulator screenshots only.

Nothing in test7 transmits. Remote ID generation is offline only (pcap on a
computer); USB HID injection and BLE/Wi-Fi transmit are not in the image.

Do **not** flash the `.img.xz` onto the 128 GB card: a full image wipes
`/data` (music, Wi-Fi, accounts). Use the `.zab`.

## 0. Update in place (128 GB card, now test6 in slot B)

The update goes into the slot that is not running: **slot A (test3) is
replaced by test7**; test6 in slot B stays as the fallback.

```
cd ~/Downloads && shasum -a 256 zpod-v0.12.0-br-ab-test7.zab
```

Compare with `SHA256SUMS`. Then:

```
cd ~/cb/projects/zpod && git fetch && git checkout test7-r1
```

```
scripts/zpod-ab-push --verify-only ~/Downloads/zpod-v0.12.0-br-ab-test7.zab
```

```
scripts/zpod-ab-push ~/Downloads/zpod-v0.12.0-br-ab-test7.zab
```

Expect "next boot runs system a (v0.12.0-br-ab-test7) on trial". On the
ZPOD: **Power > Reboot**. After 2-3 minutes:

```
scripts/zpod-ab-push --status
```

- [ ] `slot: a`, `committed`
- [ ] music, Wi-Fi, Pipit, Tidepool unchanged
- [ ] new apps installed once: **Tools > USB Lab**, **Tools > BLE Lab**,
      **Tools > Radio > Wi-Fi Lab**, **Generative > Synth**
      (`/data/log/apps-update.log` shows `zpod-bundled-new: installed ...`)
- [ ] Fallback if needed: hold **Square** while switching on -> test6.

## 1. Library (MPD state on /data)

- [ ] Play a track, pause mid-way, Power > Reboot: queue and position are
      back after boot.
- [ ] Mac: `scripts/zpod-books-push.sh -n ~/Books/ZPOD` lists a plan and
      writes nothing; without `-n` it pushes and every file says verified.

## 2. USB Lab (Tools)

- [ ] In computer-link mode it asks to switch USB to devices (Wi-Fi SSH
      warning shown). Accept.
- [ ] Plug a USB keyboard (OTG adapter or hub): it is listed as HID
      keyboard; OK shows descriptors, HID collections, endpoints, driver.
- [ ] ND-105C GPS (or any device) shows with a recognised name.
- [ ] Square saves a report: `/data/zpod/appdata/usb-lab/usb-<time>.txt`
      and `.json`.
- [ ] Triangle gives the port back to the computer; SSH over USB returns.

## 3. BLE Lab (Tools)

- [ ] Devices appear within ~30 s with signal and count; an iPhone /
      AirPods / AirTag shows an Apple Continuity class.
- [ ] OK: AD fields and raw hex. Triangle changes sort.
- [ ] Square on, wait, Square off: a `.jsonl` in
      `/data/zpod/appdata/ble-lab/`.
- [ ] (If a drone with Bluetooth Remote ID is at hand) it is listed first
      with id, position and height.

## 4. Wi-Fi Lab (Tools > Radio)

- [ ] No adapter: "Plug in a USB Wi-Fi adapter" with the powered-hub and
      "SSH stays up over Wi-Fi" text. Onboard Wi-Fi is never offered.
- [ ] RT5370 / RT3070 / MT7601U on a **powered hub**: adapters screen shows
      the onboard radio (stays on your network) and the USB one
      `monitor ok` (`, 5 GHz` on a dual-band adapter).
- [ ] OK: listening on channel 6; frame / beacon counters climb.
- [ ] Triangle: sweep 1 / 6 / 11 (plus 44 / 149 on a dual-band adapter).
- [ ] Square on/off: `wifi-<time>-ch6.pcap` in
      `/data/zpod/appdata/wifi-lab/` opens in Wireshark on the Mac.
- [ ] X: back to adapters; `iw dev` shows the USB adapter in managed mode;
      the ZPOD stayed reachable over its onboard Wi-Fi throughout.
- [ ] (If a drone with Wi-Fi Remote ID is at hand) one row per aircraft,
      updating live; OK shows the detail page.
- Optional, no radio: generate a TEST track on the Mac
  (`cd apps && cargo run -p zpod-wlan --example rid-test-pcap -- /tmp/t.pcap`),
  copy it to the ZPOD and run `ZPOD_WLAN_REPLAY=/tmp/t.pcap
  /data/zpod/apps/wifi-lab/bin/wifi-lab` from SSH: `ZPOD-TEST-0001` moves.

## 5. Synth (Generative)

- [ ] Up/Down changes voice; OK auditions it on the ZPOD speaker/DAC.
- [ ] Leave and reopen: the voice and channel are kept
      (`/data/zpod/appdata/synth/synth.conf`).
- [ ] USB MIDI keyboard (host mode): notes play, velocity audible, Square =
      silence.
- [ ] Tools > USB > **USB MIDI with computer** on (default off): the Mac's
      Audio MIDI Setup shows "ZPOD" (2 in / 2 out); GarageBand or Trillium
      plays the synth on cable 1; SSH over USB comes back after the 1-2 s
      relink. Turn it off again: plain USB networking as before.

## Please report

Commit and boot time; anything odd in `/data/log/apps-update.log`,
`/run/zpod/labd.log`, `ab.log`; which adapter, keyboard and MIDI gear you
used; per section pass/fail.
