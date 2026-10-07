# M1 local test plan — first SD image

Goal: prove the card boots a Pi Zero W (or Pi Zero) to an HDMI console and that the image contents match what we claimed to ship.

## Materials

- Disposable microSD card
- Pi Zero **or** Pi Zero W
- Micro HDMI cable + monitor/TV
- USB OTG + keyboard (for first-boot password wizard)
- Optional: Ethernet USB gadget / known network later (not required for M1 pass)

## Write the card

1. Verify checksum: `sha256sum -c SHA256SUMS`
2. Write the `.img` (after `xz -d` if needed) with your usual imager. Confirm the target device is the SD card, not a disk you care about.
3. Do **not** expect the TFT or buttons to work on this image.

## Boot checks (pass/fail)

| # | Check | Pass looks like | Fail looks like |
| --- | --- | --- | --- |
| 1 | Power + HDMI | Console or first-run wizard on the HDMI display within a few minutes | Blank HDMI forever, kernel panic scroll, or continuous reboot |
| 2 | Hostname | System presents as `zpod` (prompt / `hostname`) | Wrong or missing hostname |
| 3 | First user | Account `pi`; wizard asks you to set a password (no default password in the image) | Mystery login with a password nobody set |
| 4 | SSH package | After password is set, `systemctl is-active ssh` is active (or sshd listening) | SSH missing when ENABLE_SSH was claimed |
| 5 | No radio dependency | Pi Zero (no Wi-Fi) still reaches the console | Boot hangs waiting for network |
| 6 | No invented hardware | `/boot/firmware/config.txt` or `/boot/config.txt` has **no** `aoide-zpod-dac`, **no** `pitft22`, **no** guessed button overlays | Overlays for unknown pinouts present |

Record board revision, image filename, SHA-256, and date on the M1 issue when you run this.

## Content validation (construction side)

Before calling the Release done:

- [ ] Artifact name matches `IMG_NAME` / documented pattern
- [ ] `SHA256SUMS` published beside the image
- [ ] pi-gen commit recorded in BUILD.md (or the Release notes)
- [ ] This repo commit recorded in the Release notes
- [ ] Config used for the build archived in the Release notes or as a gist/snippet (redact nothing secret; there should be no secrets)

## Out of scope for M1 pass

TFT, buttons, DAC tone, Wi-Fi association, Bluetooth, app/media install path, BitChat, SLSA level claims.
