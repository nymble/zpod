# ZPOD firmware requirements

Owner: [nymble](https://github.com/nymble) (`nymble@gmail.com`). Repo: [nymble/zpod](https://github.com/nymble/zpod).

This document is the product requirements for a new image family for the uGeek ZPOD (Raspberry Pi Zero / Zero W portable device with 2.2" TFT, buttons, DAC, battery). It is not a rebuild of the 2017 SourceForge images.

## Target (what "done" looks like)

A portable multifunction Linux platform that:

1. Boots reliably from SD on Pi Zero and Pi Zero W (armhf).
2. Uses the onboard display and physical buttons as the primary UI for choosing among many features/apps.
3. Accepts new applications and media without reflashing the card.
4. Supports Wi-Fi and Bluetooth, including network analysis / scanning apps (and optionally a second USB radio for dedicated scan).
5. Plays audio through the onboard DAC / headphone path.
6. Can be updated and reloaded from a laptop (USB first) or the network (Zero W).
7. Publishes images with checksums, and later SBOM + SLSA provenance.
8. Remains a usable composition-analysis / SDL target for USF 2026 tmodel work.

Game emulation is desired but secondary. "Rubber duck" style HID / BadUSB-style tooling is requested as an optional app class, with clear safety boundaries. BitChat (or equivalent mesh / Bluetooth chat) is a candidate app, not a boot requirement.

## Non-goals (first releases)

- Byte-for-byte recreation of the 2017 RetroPie stock images.
- Inventing TFT, button, DAC, or battery pinouts that are not verified on hardware.
- Claiming SLSA Build L3 before the pipeline earns it.
- Making Wi-Fi or Bluetooth required for boot (breaks Pi Zero).
- Using `elbmyn` for this repo; publishing and issues are `nymble` only.

## Hardware facts (verified vs open)

Verified (2026-10-03, vendor trees):

- Boards: Pi Zero (no radio) and Pi Zero W (onboard Wi-Fi/BT).
- DAC: TI PCM5122, I2S, I2C `0x4d` on i2c1; overlay name `aoide-zpod-dac`. Not `hifiberry-dac`.
- Early display path: micro HDMI (available now for testing).
- Vendor names TFT overlay `pitft22` and 320x240; speed/fps/spi disagree across files; panel chip and GPIO map unknown.

Open / blocked:

- TFT controller and GPIO map.
- Button GPIO map (vendor scripts disagree).
- Battery / fuel gauge.
- Exact 2017 userspace feature set (needs introspection of a working card Paul already has, or a rescued stock image outside git).

## Capability requirements

### Boot and base OS

- R1. armhf image for BCM2835; hostname `zpod`.
- R2. HDMI console + serial for early bring-up.
- R3. SSH reachable once networking exists.
- R4. Pi Zero boots with no wireless interface present.

### Kernel and drivers

- R5. Prefer Raspberry Pi OS / pi-gen stock kernel + device-tree overlays. Do not start a custom kernel tree until a measured gap requires it (out-of-tree module that cannot be built against the stock kernel, or missing config for DAC/display/USB gadget).
- R6. Ship / enable `aoide-zpod-dac` once audio milestone starts; document module source and license.
- R7. TFT and buttons land only after pinouts are recorded from hardware introspection.

### Display and input

- R8. Local display is primary UX for selecting among many features.
- R9. Face and side buttons drive navigation / selection once mapped.
- R10. HDMI remains a fallback for development.

### Apps, media, updates

- R11. Writable storage for media and apps.
- R12. Copy media from laptop over USB; over network when available.
- R13. Install / remove apps without reflashing.
- R14. Update path that does not rewrite the whole card for every change; failed update leaves prior boot working.

### Radio and analysis

- R15. Zero W: Wi-Fi and Bluetooth usable for normal networking.
- R16. First-class support for Wi-Fi and Bluetooth scan / analysis apps (monitor-mode / packet tools only where legal and hardware-capable).
- R17. Optional second USB Wi-Fi or BT adapter for dedicated scanning while onboard radio stays associated.
- R18. Evaluate BitChat (or successor) as an installable app; document license, radio needs, and UI fit. Not a blocker for M1–M5.

### Secondary features

- R19. Game emulation (RetroPie-class or lighter) after core platform is usable.
- R20. Rubber-duck / USB gadget HID tooling as an optional, clearly labeled app with explicit consent and no silent enablement.
- R21. Small SDL sample for display+audio, for USF 2026 / teaching use.

### Build, publish, assurance

- R22. Scripted pi-gen build; no flash from the build scripts.
- R23. GitHub Releases + Pages landing with download and status.
- R24. Checksums first; then SBOM; then SLSA provenance ladder (no premature L3 claim).
- R25. Local test plans per milestone.

## Decision: rebuild stock features first?

**No.** Treat the 2017 image as a reference artifact to introspect, not as the first build target.

Reasons:

1. Stock images are incomplete offline (pending deletion on SourceForge; not in this repo).
2. Pinouts and overlays already conflict across vendor scripts; copying the old image does not resolve that.
3. The product goal is a modern, updateable app platform with RF tooling, not a frozen RetroPie snapshot.
4. A thin HDMI+SSH+DAC baseline is faster to test and safer to iterate.

**Yes to introspection:** when Paul can attach a working card or copy of a stock image, document packages, overlays, modules, and startup scripts into `docs/stock-introspection.md` and turn gaps into issues. That drives requirements; it does not force the first image to match.

## Kernel starting place

| Option | When | Recommendation |
| --- | --- | --- |
| Stock RPi kernel from pi-gen | Default | **Start here** |
| Out-of-tree module against stock kernel | DAC / vendor drivers | Prefer this over a full kernel rebuild |
| Custom kernel config / tree | Only if stock config cannot enable needed features | Defer until measured |

## Traceability

Every requirement above maps to a milestone issue in the repo. Changes to requirements happen in PRs to this file and comments on the parent plan issue.
