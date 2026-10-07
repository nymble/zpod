# ZPOD development plan

Parent tracking: [#1 PLAN](https://github.com/nymble/zpod/issues/1).

## Answers up front

**Best kernel starting place:** Raspberry Pi OS stock kernel from **pi-gen** (armhf, master). Add device-tree overlays and out-of-tree modules only when needed. Do **not** open a custom kernel tree for M1.

**Do we build a kernel?** Not for the first milestones. Build the *image* (rootfs + boot firmware + stock kernel). Rebuild a kernel only after a measured gap (missing config, module ABI, or gadget/monitor mode that stock cannot provide).

**Final target:** A portable multifunction armhf platform: local display + buttons as the feature selector, easy app/media load, USB/network updates, Wi-Fi/BT including scan/analysis (optional second USB radio), audio via PCM5122, secondary game emulation, optional BitChat and rubber-duck apps, published with checksum → SBOM → SLSA.

**Rebuild exact 2017 features first?** **No.** Introspect any working card Paul has and document it; build a thin new baseline first.

**BitChat?** Candidate installable app after radio stack works. Evaluate license, BLE needs, UI on 320x240, and power. Not a boot blocker.


## Rapid track (2026-10-06 PT)

M1 lite image **hardware-passed** on Pi Zero 2 W (HDMI, user `paul`, console). Resequence for usable portable device:

1. **M1.1** lighten (#13) **in parallel with** INTROSPECT (#12) on the live board.
2. **RAPID** (#14): critical path **M4 display** after verified pinouts; **M2 / M3 / M5** parallel; pull **M5** transfer+update forward.
3. **Kali policy (locked):** no Kali base image. Stay pi-gen **armhf** lite; cherry-pick tools from Debian/RPi first. No desktop/Pixel default; prefer framebuffer/DRM for TFT.
4. Suggested releases: `v0.2.0-m1.1-lite`, then `v0.3.0-portable-slice` when display **or** transfer lands.

M3 includes association **plus** low-level radio interfaces (`iw`, `btmon`, etc.). Heavy RF apps / 2nd adapter / BitChat stay M8 (#10).

## Stages

| Stage | Name | Outcome |
| --- | --- | --- |
| M0 | Publish | Pages landing, status from issues, empty download slot |
| M1 | First SD image | HDMI + serial + SSH, local test plan, Release with checksum |
| M2 | Audio + reload | `aoide-zpod-dac`, test tone, USB (then Wi-Fi) reload without full reflash |
| M3 | Radios | Zero W / Zero 2 W Wi-Fi/BT join + low-level tooling; Zero still boots offline; heavy scan apps → M8 |
| M4 | Display + buttons | HDMI until pinouts verified; then TFT + button map from hardware |
| M5 | Apps + media | Writable store, copy from laptop/network, install/update without reflash |
| M6 | Assurance | Pinned sources, SBOM, SLSA ladder (honest levels) |
| M7 | Teaching / tmodel | Image+SBOM handoff; SDL sample |
| M8 | RF toolkit | Wi-Fi/BT analysis apps; optional second adapter; BitChat evaluation |
| M9 | Secondary UX | Emulation; rubber-duck optional app with safety gates |

## Issue set (create these)

Opened on 2026-10-06 as **nymble**:

1. [#1 PLAN](https://github.com/nymble/zpod/issues/1) — parent requirements and staged build
2. [#2 M0](https://github.com/nymble/zpod/issues/2) — Project page, status, download slot
3. [#3 M1](https://github.com/nymble/zpod/issues/3) — First bootable SD image and local test plan
4. [#4 M2](https://github.com/nymble/zpod/issues/4) — Audio kernel path and USB/Wi-Fi reload
5. [#5 M3](https://github.com/nymble/zpod/issues/5) — Wi-Fi and Bluetooth on Pi Zero W only
6. [#6 M4](https://github.com/nymble/zpod/issues/6) — Display and buttons without guessing pinout
7. [#7 M5](https://github.com/nymble/zpod/issues/7) — Media, apps, and updates
8. [#8 M6](https://github.com/nymble/zpod/issues/8) — Repeatable build, SBOM, SLSA
9. [#9 M7](https://github.com/nymble/zpod/issues/9) — USF 2026 tmodel + SDL sample
10. [#10 M8](https://github.com/nymble/zpod/issues/10) — RF toolkit, second adapter, BitChat evaluation
11. [#11 M9](https://github.com/nymble/zpod/issues/11) — Emulation (secondary) and rubber-duck optional app
12. [#12 INTROSPECT](https://github.com/nymble/zpod/issues/12) — Document stock/working image contents
13. [#13 M1.1](https://github.com/nymble/zpod/issues/13) — Lighten image for portable boot
14. [#14 RAPID](https://github.com/nymble/zpod/issues/14) — Usable portable slice (display + transfer + radios)

Sub-issues stay open until their Done criteria are met. Comments and PRs carry design decisions; do not leave important artifacts only in chat.

## First concrete work (after issues exist)

1. Expand README; add `docs/` Pages stub.
2. Keep `/workspace/zpod-image` build scaffold aligned; push useful pieces into this repo (no remote was required earlier; this public repo is now the home).
3. Introspect when Paul provides a card image or package list.
4. Do **not** start a multi-hour image build or flash an SD until Paul asks.

## Safety and identity

- Publish and file as **nymble** only (not **elbmyn**).
- Build scripts never flash cards.
- Do not invent pinouts.
- Rubber-duck and RF monitor tools ship disabled / opt-in with clear docs.

## BitChat evaluation notes (2026-10-05)

- Primary projects: [permissionlesstech/bitchat](https://github.com/permissionlesstech/bitchat) (iOS/macOS, Unlicense) and [permissionlesstech/bitchat-android](https://github.com/permissionlesstech/bitchat-android) (Android; store listing notes public domain / check LICENSE in tree).
- Protocol: BLE mesh offline + optional Nostr online; whitepaper in the iOS repo.
- On ZPOD: there is no native Linux client in those repos. Path is (a) protocol-only port + BlueZ BLE central/peripheral, or (b) wait for a Linux/embedded client. Track under M8; do not block M1–M5.
- UI must fit 320x240 and button navigation; power/duty cycle matters on battery.
