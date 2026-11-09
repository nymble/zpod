# ZPOD handoff (2026-11-08, generative lane HELD)

**This is the canonical handoff.** It lives on branch `pipit-seq` at
<https://github.com/nymble/zpod/blob/pipit-seq/HANDOFF.md>. The copy on
`buildroot` is only a pointer to this one.

## Status: HELD (generative lane parked, 2026-11-08 evening)

Paul paused development on 2026-11-08 (17:47 PST), then the same evening
asked for a cleanup pass and for the generative work (Pipit, Tidepool,
zpod-seq, zpod-voices, zpod-midi, shared zpod-music-core changes) to be
**held** while the project moves to a new focus. Nothing in this lane is
being developed now. Do not build, install, flash, merge, release or publish
in this lane unless Paul asks; the "Candidate next work" list below is the
resume point.

### Hold record (2026-11-08, 21:00-22:00 PST)

- **Code:** cleanup commit on `pipit-seq` after 93870ac: the two Linux-only
  libc calls (`F_SETPIPE_SZ` in music-core audio, `SYS_gettid` in the MIDI
  worker) are behind `cfg(target_os = "linux")`, so the generative library
  crates now test on the Mac host; device behavior is unchanged. Gadget
  script comment points at the real test (`tests/usb-gadget-midi.sh`).
- **Branches:** `pipit-seq` (this lane, held) is merged into `buildroot`;
  `main` was fast-forwarded to `origin/main` (305d69f) before any work on it
  or PR #33.
- **Device (128 GB card, USB, read-only):** test6 (`1497603`) on p3/slot B.
  Installed Pipit 0.3.0 files (`bin/pipit`, manifest, icon, voices) are
  byte-identical to `pipit-0.3.0.zpa` (binary sha256 `bcd36861...ff4b683`):
  the installed-binary question is closed. Tidepool 0.1.0 also installed.
  The USB host key equals the trusted Wi-Fi key for this card
  (`HostKeyAlias=192.168.1.251` with strict checking works); the stale
  `10.12.194.1` entry in `~/.ssh/known_hosts` is another card's.
- **Tests:** full workspace in the `zpod-br` container (2026-11-09):
  **306 passed, 0 failed** on 31b004d (same tree as `buildroot` 6195034).
  Mac host, `cd apps && cargo test -p zpod-seq -p zpod-voices -p zpod-midi
  -p zpod-music-core`: 65 passed. `tests/usb-gadget-midi.sh` (offline): PASS.
- **Build VM fixed (2026-11-09):** on 2026-11-08 every VZ guest on the mini
  hung in EFI (even a minimal lima VM). The cause was the Claude desktop
  app's VZ VM, which had been stuck at ~390% CPU since Nov 6. Once the app
  was force-quit, `zpod-br` booted. The VM root disk was recreated (`colima
  delete` without `--data`, same profile); the data disk (container,
  toolchain, `/br`, 14 GB Buildroot output) was kept. Backups of the old VM:
  `~/zpod-backups/colima-zpod-br-20261108/` (APFS clones; delete when no
  longer needed).
- **Builds (container, `scripts/zpod-dev --build-only`):**
  `pipit-0.3.0.zpa` is byte-identical to the original 919141b package
  (`d2a83cec...0048`); the Pipit binary equals the installed one
  (`bcd36861...`). `tidepool-0.1.0.zpa` sha256 `3aec820e...2b4c`. Both are in
  `~/zpod-backups/generative-hold-2026-11-08/` with SHA256SUMS; earlier Mac
  host builds (different toolchain) are in its `host-build-macos/`. Nothing
  installed.
- **Still unconfirmed physically:** Pipit 0.2/0.3 controls and sound, any
  real MIDI port, NCM+MIDI composite gadget.
- **Open rule conflict:** `scripts/zpod-ssh.sh` uses
  `StrictHostKeyChecking=no` for `10.12.194.*` (pre-existing, from the dev
  loop), which contradicts the host-key rule below. Not changed; needs
  Paul's call.
- **Image impact when `buildroot` next builds:** Pipit/Tidepool are not
  bundled (image bundles music, clock, tetris, mousemusic, gps), but
  MouseMusic is rebuilt on the changed zpod-music-core (denormal flush,
  aplay stderr drain) and the gadget script gains the `midi=on` option (off
  by default). Re-check MouseMusic on the device after that build.

## How to start (Claude / next developer)

1. Read this file, then `docs/PIPIT.md`, `docs/PIPIT-DESIGN.md`,
   `docs/ZPOD-MIDI.md`, `docs/DEV-LOOP.md`, `docs/AB-UPDATES.md`,
   `docs/USB-HOST.md`.
2. On the Mac mini: `cd ~/src/zpod && git status` (do not discard anything
   you find), `git fetch origin`. New Buildroot work starts from
   `buildroot`, which contains this lane. To resume the generative lane,
   `git checkout pipit-seq` (or branch from `buildroot`).
3. Do not merge into `main` or open release work unless Paul asks. Run
   `git pull` on `main` before any work on it or PR #33.

## Branch / commit map (verified with `git merge-base --is-ancestor`)

`main` (b79793f, Pi OS track, behind origin) is separate. The Buildroot line is
linear:

```
v0.11.0-br-ab-test6 tag 1497603  (released test6 image / .zab built from here)
  -> buildroot 1ddb6d4           (qemu-boot.sh tweak + doc asset name only)
     -> tidepool-app 970b1ec     (Tidepool 0.1.0)
        -> 98da051 Pipit 0.1.0 -> 4630ba6 -> 368610b Pipit 0.1.1
        -> bb4b2c2 Pipit 0.2.0
        -> pipit-seq 919141b     Pipit 0.3.0 + zpod-midi  (current app work)
           -> (this docs-only handoff commit)
```

Other branches: `m6-ui-tools` b67eaf2 (PR #33 for `main`, unmerged).

**Released vs app-only:** the test6 release contains the code at 1497603. Tidepool
and Pipit (all versions) are **app-only `.zpa` packages** built after the
release; they are not in any released image. Don't infer app code from the image.

## Release and package evidence

- Release: <https://github.com/nymble/zpod/releases/tag/v0.11.0-br-ab-test6>
  (SHA256SUMS asset re-read 2026-11-08):
  - `2026-11-06-zpod-pi0w-v0.11.0-br-ab-test6.img.xz`
    `340f19338d678c841d5bfe26095f65ca1244c0d7de79f548cdc6bee9fc06abf1`
  - `zpod-v0.11.0-br-ab-test6.zab`
    `0dcb441b0865eab5b90a1f75fe0e77aecc600800a2772d6748149eed8b463880`
- Pipit 0.3.0 package (Mac): `~/zpod-backups/pipit-midi/dist/pipit-0.3.0.zpa`,
  sha256 `d2a83ceca9f634d786fe4f7ff93cf56471dd0f24d6359c9d00bd1754bf750048`
  (re-hashed 2026-11-08), built from 919141b. The copy **installed on the
  device** was reinstalled after the last fix and its binary checksum was not
  read back from the device: treat the installed binary as unverified until
  someone compares it on the device.
- Screenshots (simulator): `docs/screenshots/pipit-*.png` in the repo (13 files,
  used by `docs/PIPIT.md`); originals also in `~/zpod-backups/pipit-midi/shots`
  and `pm-shots.tgz`. Copies were also placed on the agent box under
  `/workspace/zpod-music/pipit-midi/` (that box is not the Mac; nothing builds
  there). Tidepool artifacts: `~/zpod-backups/tidepool-dist`, `tidepool-sim`.

## Device state (last verified 2026-11-07, not re-checked)

- uGeek ZPOD, Pi Zero W Rev 1.1, ILI9340 320x240, PCM5122 DAC.
- **128 GB card**: upgraded in place test3 -> test6 (`v0.11.0-br-ab-test6`,
  1497603). Running system p3 = slot B, committed; test3 on p2 is the fallback
  (top-level boot files). No reflash needed.
- **32 GB card**: separate; had test5 then test6 earlier.
- Media: 382 files on `/data/media/music` = 363 songs (239 mp3 + 124 flac) +
  19 artwork, 5.36 GiB, sha256-verified against the original before/after the
  migration. `/data/books` was empty at last look. Never modify these.
- Backup from before the upgrade: `~/zpod-backups/128gb-test3-20261106/` on the
  Mac (boot FAT + test3 rootfs images, music, `/data` app/core tars, hash lists).
- Installed apps: bundled clock 1.2.0, gps 0.2.0, mousemusic 0.2.0, music
  1.2.0, tetris 1.1.0; plus Tidepool 0.1.0 and Pipit 0.3.0 (`.zpa` in
  `/data/zpod/apps`).

## Mac mini and builds

- Mac mini **Stellas-Mac** (Mac16,10, M4, 24 GB). Repo `~/src/zpod`.
  Builds only here, never on a shared/cloud box. GitHub CLI is logged in as
  **nymble** only; no cloud CI assumed.
- Buildroot: colima profile `zpod-br`, container `zpod-br`, repo at
  `/src/zpod`, work volume `/br`:
  `docker --context colima-zpod-br exec zpod-br sh -c 'cd /src/zpod && ZPOD_BR_WORK=/br buildroot/build.sh'`
  -> `/br/output/images/sdcard.img`.
- Apps: `scripts/zpod-dev --build-only <id>` -> `apps/dist/dev/<id>-<ver>.zpa`;
  `scripts/zpod-dev <id>` builds + installs over SSH (no reflash);
  `scripts/zpod-dev --core` for launcher/`zpod-app`. Underneath:
  `apps/tools/build-app.sh` (`cargo zigbuild`, ARMv6 musl). See `docs/DEV-LOOP.md`.
- Workspace tests: `cd apps && cargo test` (306 passing at 919141b).
- A/B system updates: `scripts/zpod-ab-push` with a `.zab` (`docs/AB-UPDATES.md`).
- Paul flashes cards himself. Never give flash/`dd` instructions until he has
  confirmed a disposable target card.

## Apps and shared crates (`apps/crates`)

- Pipit 0.3.0: 16-step, 5-track groove sequencer. LIVE screen, tap tempo,
  per-track rate, mute/solo, voice browser (20 voices, 5 kits), per-track
  macros, echo/drive, optional MIDI out. Pattern schema v2 (v1 migrated on load;
  the original pattern is kept). MIDI routing defaults to Internal; `midi.toml`
  is written only when the MIDI setup screen is closed. Physical buttons and
  listening were confirmed by Paul only for 0.1 (alpha); 0.2/0.3 controls and
  sound are **not** physically confirmed.
- Shared crates: `zpod-seq` (events/patterns), `zpod-voices` (voice rack),
  `zpod-music-core` (synth, audio out, MPD pause), `zpod-input` (evdev
  keyboards/mice), `zpod-ui` (UI kit incl. `textentry.rs`), `zpod-midi` (MIDI
  out), `zpod-core`/`zpod-hal`. Rules: portable contracts; no allocation, locks
  or file I/O on the render thread.
- MIDI today: Pipit events -> `zpod-midi` router -> lock-free queue -> worker
  writing ALSA rawmidi. Not done: real external hardware, MIDI input, a shared
  audio/MIDI broker so apps can co-run. Trill is a USB MIDI **controller**
  (sends only); it is not a synth and nothing here receives from it yet.
- Tidepool and Trill were not changed by the Pipit work. Paul parked Tidepool
  polish and DigiPi.

## Test evidence and limits

- 306 workspace tests pass (919141b). On-device selftests via the internal
  DAC: zero real ALSA underruns; CPU about 18-31% after the tiny-float
  (subnormal) fix.
- MIDI verified with a fake sink and a FIFO on the device only; no physical
  MIDI port or receiver. Output-to-audio alignment (~110 ms) is an estimate,
  not measured. Mock outputs are not physical proof.
- Event cents become channel-wide pitch bend (not MPE).
- Pipeline shutdown writes can look like xruns in older logs (0.1.1 separated
  them; see PIPIT-DESIGN "Diagnostics").
- Composite USB NCM+MIDI gadget: **not done**. The image script's `midi=on`
  is only tested offline (`tests/usb-gadget-midi.sh`); the live gadget was not
  touched and there is no GUI for it.

## Talking to the device

- USB gadget: CDC-NCM, ZPOD is `10.12.194.1`; Wi-Fi SSH at the address the
  device shows (last seen on the home LAN as 192.168.1.251). Buildroot: `ssh
  root@...` with key auth; apps run as `zpod`. Old Pi OS: no sudo.
- Host keys may differ per card and per path (USB vs Wi-Fi). Keep strict host
  key checking; use `HostKeyAlias` only for a key you already trust for that
  card. Never `StrictHostKeyChecking=no`.
- `zpod-app` is `/usr/lib/zpod/bin/zpod-app`.
- USB role switches live, no reboot: **Tools > USB** on the device (or an app
  prompt). Only Paul switches it. Known issue: after going back to the computer
  link the Mac can show the NCM interface as "connected" but stale; if the Mac
  doesn't reach 10.12.194.1, Paul switches host -> gadget again on the device.
  No unattended remote switching. Persistence (Square = keep for next boot) and
  defaults: `docs/USB-HOST.md`.

## A/B updates (custom `.zab`, not RAUC)

- `zpod-ab` writes the spare slot, reads it back, trial-boots it, and commits
  only after userspace comes up; otherwise falls back to the old slot.
  Limits: a kernel/firmware failure before userspace can't auto-roll back.
- Recovery: hold **Square** at power-on to boot the other slot (not yet tried
  physically), or delete `zpod-slot.txt` on the `ZPODBOOT` drive to boot the
  top-level system (test3 on p2 for the 128 GB card). **Home** at power-on is
  safe mode (skips `/data` overlays), independent of slots.
- The updater does not wipe `/data`, but it does write its A/B state under
  `/data`.
- `.zpa` app installs are separate from A/B and need no reflash.

## Rules (from Paul)

- Commit/publish/auth on GitHub only as **nymble**
  (`764245+nymble@users.noreply.github.com`).
- No Wi-Fi/SSH/login secrets anywhere (commits, logs, images, chat); use
  `<SSID>`/`<PW>` placeholders.
- Never flash or wipe a card from a script; never touch `/data/media/music`
  or `/data/books`, credentials or user data.
- Split steps that need Paul into separate zsh commands. Keep other project
  lanes out of this repo's work.

## Candidate next work (ordered, NOT started, needs Paul's go-ahead)

1. Physical test of Pipit 0.3 controls and sound on the device; read back
   the installed binary checksum.
2. External MIDI output on real hardware; composite NCM+MIDI gadget with a GUI
   switch, keeping the USB network working; measure timing.
3. MIDI input (Trill, keyboards).
4. Shared integration (MouseMusic/Tidepool) and an audio/MIDI broker.
5. Pipit per-step locks, pattern chains, etc.
6. App updater/manager work (distinct from the `.zab` A/B updater).

The wider wishlist is not delivered; this list is not exhaustive.

---

## Historical (2026-11-01 handoff, partly obsolete)

Kept for background. Obsolete: the "latest prerelease" (now test6), the
145-test count (now 306), "test3 on the 128 GB card" (now test6 in slot B),
and the job list (A/B updates are done and used on the 128 GB card).


A short note so another assistant or developer can pick up the build. Read
`README.md`, `PLAN.md`, `docs/BUILDROOT.md` and `docs/TEST-BR-M6.md` next.

### Where things are

- Device: uGeek ZPOD with a Raspberry Pi Zero W Rev 1.1, ILI9340 320x240 TFT,
  PCM5122 DAC (I2C 0x4d, `aoide-zpod-dac` driver), MAX17048 fuel gauge (0x36),
  DS3231 RTC (0x68). Hardware notes are in `docs/` (BUTTONS, BATTERY, USB-GADGET, ...).
- Two OS tracks:
  - `main`: Pi OS (pi-gen) image, the supported one (`build.sh`, `docs/BUILD.md`).
  - `buildroot`: the fast-boot image (`buildroot/`, `docs/BUILDROOT.md`). This is
    where current work happens. Latest prerelease:
    [v0.9.0-br-m6-fastboot-test3](https://github.com/nymble/zpod/releases/tag/v0.9.0-br-m6-fastboot-test3)
    (tag at cb94648; branch head 9ab71fc).
- Open PR [#33](https://github.com/nymble/zpod/pull/33) (`m6-ui-tools`) carries the
  test3 UI and Tools / Wi-Fi changes for `main`. PR #23 is the Buildroot draft.
- Apps are Rust in `apps/` (launcher, bootsplash, drop, zpod-battery, zpod-app,
  `.zpa` app bundles). `cargo test` passes 145 tests as of test3.

### Building

Builds run on Paul's Mac (Stellas-Mac), never on a shared/cloud box.

- Buildroot: colima profile `zpod-br`, container `zpod-br`, repo mounted at
  `/src/zpod`, work volume at `/br`:
  `docker --context colima-zpod-br exec zpod-br sh -c 'cd /src/zpod && ZPOD_BR_WORK=/br buildroot/build.sh'`
  Output: `/br/output/images/sdcard.img`. Incremental rebuilds take 1-3 min.
- Tests in the container: `sh buildroot/tests/test_firstuser_br.sh`,
  `sh buildroot/tests/test_shims_br.sh` (31/31 each), and the QEMU smoke test
  `buildroot/tests/qemu-boot.sh`.
- Release: copy the image out (`docker cp`), `xz` it, write `SHA256SUMS` for the
  .img and .img.xz, run `scripts/check-image-secrets.sh` on it, and publish a
  GitHub prerelease named `YYYY-MM-DD-zpod-pi0w-<version>.img.xz`.

### Talking to the device

- USB data port gives a CDC-NCM network: the ZPOD is `10.12.194.1`.
- Buildroot test card: `ssh root@10.12.194.1` with key auth (key from
  `zpod-ssh.pub` on the boot partition). Apps run as user `zpod` (uid 1000).
  BusyBox userland, no rsync client on the device: push files with tar over ssh
  (see `scripts/zpod-push-music.sh`).
- App dev loop (no reflash, ~4 s edit-to-screen): `scripts/zpod-dev <app-id>`;
  launcher/`zpod-app` updates from `/data`: `scripts/zpod-dev --core`. See
  `docs/DEV-LOOP.md`. On test3 the `/data` launcher overlay is lost at reboot
  and re-applied by the next `zpod-dev` run; later images apply it at boot.
- Partitions: p1 boot FAT 64 MB, p2 system A (squashfs, read-only), p3 system B
  (spare, for A/B updates), p4 `/data` (rest of card, grows on first boot).
- Wi-Fi: `nmcli` is a shim over wpa_supplicant / hostapd / udhcpd
  (`buildroot/board/zpod/rootfs-overlay`). Saved networks live in
  `/data/etc/wpa_supplicant.conf`. Seed Wi-Fi before first boot with
  `zpod-wifi.txt` on the boot partition (`SSID=`/`PSK=` lines). On-device
  `wpa_passphrase` rejects piped stdin; compute the PSK off-device.

### Rules (from Paul)

- Publish, commit and authenticate on GitHub only as **nymble**
  (`764245+nymble@users.noreply.github.com`), never as `elbmyn`.
- Never put Wi-Fi, SSH or login passwords in commits, releases, logs, images or
  chat. Docs use `<SSID>` / `<PW>` placeholders. Earlier releases were withdrawn
  for this.
- Never flash or wipe an SD card from a script. Paul confirms the target card and
  runs `dd` himself.
- Never invent pinouts; verify on hardware or from uGeek docs.
- Split steps that need Paul's input into separate commands. He uses zsh.
- UI style: tight, clean, Apple-like screens on 320x240.

### State at handoff

Test3 is on Paul's 128 GB card and working: correct version, red LED off at idle,
Wi-Fi joined home network, clock synced, Bluetooth scan, music copied (382 tracks).
Not yet checked on hardware: hotspot device count + status-bar mode icon, Wi-Fi
Channel use chart, switching back to Client mode.

### Next jobs, in order

(Done 2026-11-05: app dev loop, ROADMAP-APPS step 1, `docs/DEV-LOOP.md`.
Core 0.10.0: menu tree and manifest schema 1 (`docs/APP-MANIFEST.md`), core
updates from `/data` loaded at boot with fallback (`docs/BOOT-UPDATES.md`),
bundled apps updated at boot; test card `docs/TEST-BR-TEST5.md`.)

1. **Non-destructive updates**: done in emulation for test6 (core 0.11.0):
   `zpod-ab`, `.zab` bundles, `scripts/zpod-ab-push`, trial boot + fallback,
   in-place migration of a test5 card ([AB-UPDATES.md](docs/AB-UPDATES.md),
   [TEST-BR-TEST6.md](docs/TEST-BR-TEST6.md)). Next: Paul runs the first
   real update on test5; then signing.
2. **App manager** under Tools: add/remove apps packaged under `/data/apps`.
3. Hotspot web console for the iPad (music upload, Wi-Fi setup, status).
4. USB landing page that interrogates whatever is plugged in (Bash Bunny-style
   tools later); active Wi-Fi tools; mDNS. (Typing a Wi-Fi password on the
   device: done in 0.11.0, USB keyboard or buttons, [TEXT-ENTRY.md](docs/ui/TEXT-ENTRY.md).)
5. Merge PR #33 into `main`.
