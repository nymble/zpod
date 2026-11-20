# Test8: labs with gated TX, DuckyScript, dual license

Release `v0.13.0-br-ab-test8` (pre-release, full working image for hardware
testing). Emulation tests only so far: host/ARMv6 unit tests, Buildroot
checks, QEMU boot. **Not yet run on hardware.**

Carry-forward from test7: USB Lab, BLE Lab, Wi-Fi Lab (receive), Synth M1,
USB MIDI switch, MPD state on `/data`, books push. New in test8: Wi-Fi Lab
**TEST Remote ID transmit** (confirm + arm every time), **HID Lab**
(DuckyScript select / edit / dry-run / confirm+arm), Tools > USB **HID**
switch, dual **GPL-3.0-or-later OR commercial** license on Paul-owned crates.

Do **not** flash the `.img.xz` onto the 128 GB card: a full image wipe
destroys `/data`. Use the `.zab` in-place update.

## 0. Update in place (normal reboot — no Square held)

```
cd ~/Downloads && shasum -a 256 zpod-v0.13.0-br-ab-test8.zab
```

Compare with `SHA256SUMS`. Then:

```
cd ~/cb/projects/zpod && git fetch && git checkout test8
scripts/zpod-ab-push --verify-only ~/Downloads/zpod-v0.13.0-br-ab-test8.zab
scripts/zpod-ab-push ~/Downloads/zpod-v0.13.0-br-ab-test8.zab
```

Expect the next boot to run the new system on trial. On the ZPOD:
**Power > Reboot** (plain reboot). After 2–3 minutes:

```
scripts/zpod-ab-push --status
```

A/B fallback still exists under the hood if a trial boot fails; it is **not**
part of the happy-path update. Do not hold Square unless recovering.

## 1. Module / adapter checklist

- USB Wi-Fi (RT3070 / RT5370 / MT7601U) on a **powered hub**; SSH stays on
  onboard Wi-Fi.
- Tools > USB > **USB HID keyboard (Ducky)** on before HID Lab runs.
- Isolated lab before any transmit (cabled / dummy load / Faraday).

## 2. Feature checks (short)

| App | Check |
|---|---|
| USB Lab | Host mode: list devices, detail, save report |
| BLE Lab | Passive LE scan, classify, capture |
| Wi-Fi Lab RX | Monitor on USB adapter, sweep, aircraft list, pcap |
| Wi-Fi Lab TX | Square → safeguard → OK confirm → Triangle arm; TEST beacons 15s |
| HID Lab | Enable HID, open demo script, dry-run, confirm+arm, run on your Mac |
| Synth / MIDI | Unchanged from test7 |
| Library | Music state survives reboot; books push dry-run |

