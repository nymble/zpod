# ZPOD development plan

Parent tracking: open issue **[PLAN] New image platform — requirements and staged build** (filed from this document).

## Answers up front

**Best kernel starting place:** Raspberry Pi OS stock kernel from **pi-gen** (armhf, master). Add device-tree overlays and out-of-tree modules only when needed. Do **not** open a custom kernel tree for M1.

**Do we build a kernel?** Not for the first milestones. Build the *image* (rootfs + boot firmware + stock kernel). Rebuild a kernel only after a measured gap (missing config, module ABI, or gadget/monitor mode that stock cannot provide).

**Final target:** A portable multifunction armhf platform: local display + buttons as the feature selector, easy app/media load, USB/network updates, Wi-Fi/BT including scan/analysis (optional second USB radio), audio via PCM5122, secondary game emulation, optional BitChat and rubber-duck apps, published with checksum → SBOM → SLSA.

**Rebuild exact 2017 features first?** **No.** Introspect any working card Paul has and document it; build a thin new baseline first.

**BitChat?** Candidate installable app after radio stack works. Evaluate license, BLE needs, UI on 320x240, and power. Not a boot blocker.

## Stages

| Stage | Name | Outcome |
| --- | --- | --- |
| M0 | Publish | Pages landing, status from issues, empty download slot |
| M1 | First SD image | HDMI + serial + SSH, local test plan, Release with checksum |
| M2 | Audio + reload | `aoide-zpod-dac`, test tone, USB (then Wi-Fi) reload without full reflash |
| M3 | Radios | Zero W Wi-Fi/BT; Zero still boots offline; scan apps can start |
| M4 | Display + buttons | HDMI until pinouts verified; then TFT + button map from hardware |
| M5 | Apps + media | Writable store, copy from laptop/network, install/update without reflash |
| M6 | Assurance | Pinned sources, SBOM, SLSA ladder (honest levels) |
| M7 | Teaching / tmodel | Image+SBOM handoff; SDL sample |
| M8 | RF toolkit | Wi-Fi/BT analysis apps; optional second adapter; BitChat evaluation |
| M9 | Secondary UX | Emulation; rubber-duck optional app with safety gates |

## Issue set (create these)

1. **[PLAN] New image platform — requirements and staged build** (parent; links REQUIREMENTS.md + PLAN.md)
2. **[M0] Project page, status, download slot**
3. **[M1] First bootable SD image and local test plan**
4. **[M2] Audio kernel path and USB/Wi-Fi reload**
5. **[M3] Wi-Fi and Bluetooth on Pi Zero W only**
6. **[M4] Display and buttons without guessing pinout**
7. **[M5] Media, apps, and updates from laptop or network**
8. **[M6] Repeatable build, SBOM, SLSA provenance**
9. **[M7] USF 2026 tmodel target and SDL sample**
10. **[M8] RF scan/analysis apps, second adapter, BitChat evaluation**
11. **[M9] Emulation (secondary) and rubber-duck optional app**
12. **[INTROSPECT] Document packages/overlays/modules from a working stock or personal image**

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
