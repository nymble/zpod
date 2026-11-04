# ZPOD apps roadmap: app classes, lifecycle, dev loop, portability

Captured 2026-11-03 from Paul's request: "two more classes of feature sets for
loadable applications to include generative music; graphical generative art.
Ease of use of moving, deleting, creating, transferring applications and data
is important. Ease of development by minimization of flashing and direct
USB-Host development loop for applications. Portability is important. Are we
all Rust? Are our application bundles well defined? Use the use cases to check
the functionality." Plus the same day's future goal: every
[DigiPi](https://craiger.org/digipi/) feature.

Facts below were read from the `buildroot` branch at `02d221f` (core 0.9.0,
test3). The ZPOD was not reachable over USB during this audit (no
`10.12.194.x` interface on the Mac, ssh timed out), so nothing here was
checked on the device; evidence is the code and the docs.

Related: [APP-BUNDLE.md](./APP-BUNDLE.md) (the `.zpa` spec, v0 + proposed v1),
[APPS.md](./APPS.md), [BUILDROOT.md](./BUILDROOT.md), [STORAGE.md](./STORAGE.md),
[FILE-TRANSFER.md](./FILE-TRANSFER.md), [plan/ROADMAP.md](./plan/ROADMAP.md),
[research/BASH-BUNNY.md](./research/BASH-BUNNY.md), [../HANDOFF.md](../HANDOFF.md).

## 1. Are we all Rust?

**No, but everything users see is.** Line counts of tracked source files
(`git ls-files`, newline count, comments included):

| Language | Files | Lines | Share | What |
|---|---|---|---|---|
| Rust | 66 | 18,669 | 74 % | all of `apps/crates`: launcher, bootsplash, drop, zpod-battery, zpod-app, music, clock, tetris, and the libraries (zpod-core, -hal, -ui, -panel, -drivers, -mpd, -apps, tetris-core) |
| Shell | 67 | 3,803 | 15 % | image builds (`build.sh`, `build/`, `buildroot/build.sh`, post-build/image), tests, Mac helper scripts, and the device's init/plumbing |
| Python | 12 | 2,386 | 9 % | buttons daemon + Pi OS legacy helpers, Mac tagging tool (`scripts/zpod_tag_push.py`, 746), test helpers |
| C + DTS | 2 | 212 | 1 % | `drivers/aoide-zpod-dac` kernel module (168) + overlay (44) |
| Make | 6 | 98 | <1 % | Buildroot package `.mk`, driver Makefile |

What runs **on the device** (Buildroot image):

| Runtime | Pieces |
|---|---|
| Rust (static musl ARMv6) | `zpod-bootsplash`, `zpod-launcher` (root), `zpod-drop`, `zpod-battery`, `zpod-app`, apps `music`, `clock`, `tetris` (as `zpod`) |
| **Python, always running** | `overlay/usr/lib/zpod/zpod_buttons.py` (408 lines): GPIO polling via `/dev/gpiomem`, uinput "zpod-buttons" keyboard, the only ALSA volume writer. Started by `S30zpod-hw` with `python3` |
| Python, occasionally | `zpod-wpa-psk` (35 lines): derives the WPA PSK when a `zpod-wifi.txt` is imported |
| Python, installed but unused on Buildroot | `zpod_hw.py` (201; only imported by `zpod_ui.py`, which the Buildroot image does not install) and `zpod_battery_guard.py` (88; replaced by Rust `zpod-battery`). Both are in `ZPOD_SCRIPTS_FILES` (`buildroot/package/zpod-scripts/zpod-scripts.mk`) |
| Shell (BusyBox sh), ~1,020 lines | `rcS`, `S00zpod-data` .. `S61zpod-rsyncd`, `zpod-data-early/-grow`, `zpod-firstuser-br`, `zpod-wifi-import-br`, `zpod-launcher-early/-run/-stop`, `zpod-bootmark`, and the **shims** `nmcli` (243) and `systemctl` (51) that the Rust Wi-Fi tools call; plus `zpod-usb-gadget.sh` (74) and `zpod-audio-init.sh` (21) |
| C | `aoide-zpod-dac.ko` |

So the image still ships `python3` + python-evdev + smbus2 only for the
buttons daemon and the PSK helper. Porting both to Rust (already next step 2
in BUILDROOT.md) removes Python from the image and saves seconds of boot on a
Zero W. The shell init and shims are reasonable as BusyBox glue; the `nmcli`
shim is the one piece large enough to be worth moving into `zpod-hal::wifi`
(call `wpa_cli`/`hostapd` directly) later.

## 2. Are the app bundles well defined?

**Implemented well, specified only partly.** See [APP-BUNDLE.md](./APP-BUNDLE.md)
section 1 for the full gap table. In short, the `.zpa` (gzip tar,
`manifest.toml` first) has strict, tested install code: staged atomic swap,
path/symlink/device rejection, size limits, ARMv6 ELF check, `min_core`,
`deny_unknown_fields`. Missing: a format version, app kinds (no background
services), declared resources/permissions (every app gets spi/gpio/i2c/audio),
a data-dir policy (remove leaves data, no backup/export/migration), multi-target
variants, signing, per-file hashes, and dev-mode tooling.

## 3. App classes

| Class | Examples | Needs from the platform | Status |
|---|---|---|---|
| **Built-in media/time/games** | Music (MPD), Clock (alarm, timer), Tetris | LCD, buttons, MPD, storage | shipping as `.zpa` (`apps/packages/*`) |
| **Tools** | Wi-Fi tools, Bluetooth scan, USB landing page, Battery, File transfer, App manager | network, radios, root helpers | inside the launcher today (`launcher/src/{tools,wifitools,usb,transfer}.rs`); new tools should become `kind = "tool"` bundles where they do not need root |
| **Generative music** (new) | drone/ambient generators, step sequencer, Euclidean rhythms, granular player on `/data/media`, "music for airports" style long-form | low-latency PCM out through the PCM5122 (`AudioSink`), audio arbitration with MPD, button-to-parameter mapping, presets in the data dir, optional render-to-file into the music library | trait only: `zpod_core::AudioSink` exists, **no implementation**; MPD opens `hw:CARD=sndrpiaoidezpod` directly and `/etc/asound.conf` has no dmix |
| **Generative art** (new) | flow fields, cellular automata, L-systems, reaction-diffusion, screensaver/ambient clock faces, audio-reactive visuals (with the music class) | a frame loop with a target fps and dirty-rect flush, RGB565 framebuffer, fixed-point/fast math on ARMv6 (no NEON), seed + preset save, PNG snapshot export | building blocks exist: `zpod_ui::fb::Framebuffer`, `zpod_hal::lcd` flushes changed regions; full 320x240 frame is ~150 kB, ~40 ms at 32 MHz SPI => **~20-25 fps ceiling for full-screen change** |
| **Radio / DigiPi set** (future goal) | APRS (Direwolf: digipeater, IGate, webchat, map), Winlink (Pat + ARDOP/Mercury), FT8, JS8Call, FLDigi, SSTV, AX.25 node/BBS, Bluetooth KISS TNC, rig control (hamlib), web console + Wi-Fi autohotspot | **audio in** (USB sound card / DigiRig), **PTT** (serial RTS/CAT or CM108 GPIO), **USB host mode**, background services, a browser UI over the hotspot, accurate time (FT8: GPS or NTP) | missing; see section 7 |

## 4. App and data lifecycle

Where things live on Buildroot (`buildroot/board/zpod/post-build.sh`,
`zpod-launcher-run`, STORAGE.md): apps in `/data/zpod/apps/<id>` (via the
`/opt/zpod/apps` link), app data in `/data/zpod/appdata/<id>`, media in
`/data/media/{music,playlists}` (also `/srv/zpod/media`), books `/data/books`,
uploads `/data/inbox`. All on p4, so they survive a future A/B system update.

| Operation | Today | Target |
|---|---|---|
| Install / update | `zpod-app install|update` over ssh; `scripts/zpod-app-push.sh` (scp + ssh + sudo) | same, plus App manager "Install from inbox" (a `.zpa` uploaded through the drop box or web console), and `zpod-app dev` |
| Remove | `zpod-app remove <id>` removes the app dir only; data stays in `appdata/<id>` | App manager: Remove / Remove + data (confirm dialog); `zpod-app remove --purge` |
| Move (re-order, hide) | `order` in the manifest only | App manager: move up/down, hide; stored in `/data/etc/zpod/apps.conf`, not in the bundle |
| Create | copy an `apps/crates/<x>` crate by hand, add a `packages/<x>/manifest.toml`, edit `build-arm.sh` | `zpod-app new <id> --template art|music|tool` generates a crate + manifest outside the core workspace |
| Transfer app to another ZPOD / back to Mac | not possible except re-pushing the original `.zpa` | `zpod-app export <id>` rebuilds a `.zpa`; web console "Download app" |
| Back up / restore data | manual `tar` over ssh | `zpod-app backup <id|--all>` and `restore`; web console download; included in a pre-update snapshot |
| Media (music) | `scripts/zpod-push-music.sh` (rsync over ssh), drop box `PUT /music/...` and browse/download (`drop/src/main.rs`), rsync share | web console upload/download from iPad; MPD rescan already automatic |
| System update | reflash (p3 is a stub) | RAUC-style A/B into p2/p3, `/data` untouched, tryboot fallback (HANDOFF next job 1) |

## 5. Development loop (minimise flashing)

Today: cross-build with `cargo zigbuild` (`apps/tools/build-arm.sh`, builds all
eight binaries, works on the Mac), `zpod-app pack`, then
`scripts/zpod-app-push.sh`. Friction found in the code:

1. `zpod-app-push.sh` logs in as `pi@` (Pi OS user). On Buildroot the user is
   whatever `userconf.txt` created (or root with key only), so the script needs
   `-u user` / `ZPOD_USER` and should skip `sudo` when it is root.
2. There is no way to start, stop or restart an app remotely. The launcher is
   the LCD owner and launches only from a button press; running the binary
   over ssh would fight it for spidev.
3. App output goes to the launcher's log (`/run/zpod/launcher.log`), mixed
   with launcher lines; no per-app log, no `tail -f` per app.
4. `build-arm.sh` rebuilds and repacks everything; there is no per-app target.
5. The simulator (`ZPOD_SIM=<dir>`, `zpod-hal/src/sim.rs`) is headless and
   scripted (PNG screenshots), and works "on any Linux host" only:
   `cargo check -p zpod-hal` **fails on macOS** (2026-11-03, on the Mac:
   `linux-embedded-hal` has `#[cfg(target_os = "linux")]` modules). There is no
   interactive window.

Target loop (**USB cable, no reflash, < 30 s** edit-to-screen):

```
Mac: zpod-app dev apps/myart            # or: scripts/zpod-dev apps/myart
  -> cargo zigbuild -p myart --release --target arm-unknown-linux-musleabihf
  -> zpod-app pack (host build) -> scp over CDC-NCM 10.12.194.1
  -> ssh: zpod-app install --dev --force; zpod-app restart myart
  -> ssh: zpod-app logs -f myart      (Ctrl-C leaves the app running)
```

Pieces: a launcher control socket (`/run/zpod/launcher.sock`: `launch <id>`,
`stop`, `restart`, `status`), per-app logs, `--dev` installs (unsigned
allowed, "DEV" badge), and `zpod-app dev` on the Mac. "Hot reload" here means
**restart in place**: the launcher stops the old process group and execs the
new binary, with data dir intact. True in-process hot reload is not worth it
for static binaries.

**"Direct USB-Host development loop"**: the Mac is the USB host and the ZPOD
the gadget (CDC-NCM) today, which is exactly the dev link. Note the conflict
with the radio set: the Zero W has **one** USB data port. When it is switched
to host mode for a DigiRig/sound card, the CDC-NCM link is gone, so the dev
loop must also work over Wi-Fi (same commands, `-H <ip>` or `zpod.local`
once mDNS exists).

Desktop target: make `zpod-hal` build on macOS by gating the spidev/gpio/evdev
backends behind `cfg(target_os = "linux")` and adding a `sim` backend with an
optional window (e.g. `minifb`, keyboard mapped to ZPOD keys, a PCM output
via `cpal` for generative music). Same app crate, `cargo run -p myart
--features sim-window` on the Mac.

## 6. Portability

| Target | Status | Notes |
|---|---|---|
| Pi Zero W (ARM1176, ARMv6, 512 MB) | the platform | apps: static `arm-unknown-linux-musleabihf`; installer refuses non-ARMv6 ELFs (`zpod-apps/src/elf.rs`) |
| Pi Zero 2 W (Cortex-A53, 4 cores) | apps: yes in principle (ARMv6 code runs on ARMv7/AArch32); **Buildroot image: no** | `zpod_pi0w_defconfig` targets ARM1176 with the `bcmrpi` kernel (BCM2835 only). Needs a `zpod_pi02w_defconfig` (`bcm2709` 32-bit or `bcm2711`-style 64-bit kernel, same rootfs layout). The Pi OS track's M1 passed on a Zero 2 W. Needed for heavy DigiPi modes (DigiPi does not support single-core Pis) |
| Desktop simulator (Linux) | headless, scripted, PNG | `ZPOD_SIM`, `apps/tools/sim/*.txt`, `sim-apps.sh` |
| Desktop simulator (macOS) | **does not compile** | see 5.5 |
| ESP32-S3 / bare metal | libraries only | `no_std` cores checked by `apps/tools/check-nostd.sh` (APPS.md "Portability") |

App API that hides the hardware: the `zpod-core` traits (`InputSource`,
`Clock`, `Storage`, `Battery`, `AudioSink`) plus `zpod_hal::Platform` (display
+ input) already do this for display, buttons and storage. To finish it:
implement `AudioSink` for ALSA (device) and the simulator (host), add an
`AudioSource` trait (radio set), add a frame-timing helper for animation, and
publish a small `zpod-sdk` crate that re-exports these so third-party apps do
not depend on the internal crate layout. Apps should never open spidev, GPIO
or I2C directly (the `[requires] gpio/i2c` flags in APP-BUNDLE v1 make the
exception explicit).

## 7. DigiPi feature set (future goal)

| DigiPi feature | Software | ZPOD needs | Zero W? |
|---|---|---|---|
| APRS digipeater / IGate / webchat / map | Direwolf + web UI | audio in/out, PTT, USB host, service apps, web console | Direwolf 1200 baud is light; expected OK, **unverified** |
| Winlink server + Pat (ARDOP / Mercury) | Pat, ardopc, Mercury | same + more CPU | ARDOP marginal; Mercury: Zero 2 W |
| FT8 / JS8Call / FLDigi | wsjtx/jt9, js8call, fldigi | audio, time sync, X/VNC or web UI | Zero 2 W (DigiPi also requires multi-core) |
| SSTV | qsstv / slowrx | audio | decode feasible, UI heavy |
| AX.25 node / BBS | linbpq / ax25 tools | kernel AX.25 (not in `linux.fragment`), KISS | OK |
| Bluetooth KISS TNC | Direwolf + rfcomm | BlueZ (present) | OK |
| Rig control | hamlib `rigctld` | serial over USB host | OK |
| Web console + Wi-Fi autohotspot | web server | hotspot (shim exists, untested on hardware), mDNS | planned (HANDOFF job 3) |

Hardware blockers on the ZPOD: no audio input (PCM5122 is output only), no PTT
line, and the data port is `dr_mode=peripheral` (`buildroot/board/zpod/config.txt`)
with USB host drivers built only as modules. Plan: a "Radio" image variant or
runtime switch to `dr_mode=otg`/host, a DigiRig (USB audio + serial PTT), and
`kind = "service"` bundles for Direwolf/Pat with a web page each.

## 8. Use cases checked against the code

Status: **works** (implemented and documented; not re-run on hardware in this
audit), **partial**, **missing**.

| # | Use case | Status | Evidence | Missing piece |
|---|---|---|---|---|
| U1 | Paul copies a new generative-art app from his Mac and runs it without reflashing | partial | `zpod-app install` (`package.rs`), `scripts/zpod-app-push.sh`; apps live on `/data` (`post-build.sh`) | push script hard-codes `pi@` (wrong user on Buildroot); no template; must start it by hand on the device |
| U2 | Remove an app **and** its data | partial | `zpod-app remove` (`package.rs::remove`) deletes `/data/zpod/apps/<id>` only | `--purge` / data policy; `appdata/<id>` is left behind |
| U3 | Install / remove / reorder apps on the device from Tools | missing | launcher Page::Apps is a read-only list (`launcher/src/main.rs`) | App manager (HANDOFF job 2) |
| U4 | Edit code on the Mac and see it on the ZPOD over USB in under 30 s | partial | `build-arm.sh` (cargo-zigbuild), pack, push; CDC-NCM link | per-app build, launcher control socket (launch/restart), per-app logs, `zpod-app dev` |
| U5 | Try an app on the Mac with no ZPOD attached | partial | `ZPOD_SIM` headless sim (`zpod-hal/src/sim.rs`), Linux only | `zpod-hal` fails `cargo check` on macOS; no window, no audio |
| U6 | Run a generative music app through the DAC with button control | missing | `AudioSink` trait only (`zpod-core/src/lib.rs`); MPD holds `hw:` (`rootfs-overlay/etc/mpd.conf`); no dmix (`overlay/etc/asound.conf`) | ALSA `AudioSink`, MPD pause/resume around exclusive audio apps, reference app |
| U7 | Run a full-screen generative-art animation with button control | partial | `Framebuffer` + changed-region flush (`zpod-hal/src/lcd.rs`), Tetris animates | frame-timing helper, fps budget doc (~20-25 fps full-frame), reference app, PNG export of a frame |
| U8 | Move music from the Mac to the ZPOD | works | `scripts/zpod-push-music.sh` (rsync over ssh), drop `PUT /music/...` (`drop/src/main.rs`), FILE-TRANSFER.md | - |
| U9 | Move music between ZPOD and iPad over the hotspot | partial | drop box browse/download/upload (off by default), hotspot via `nmcli` shim + hostapd (BUILDROOT.md: untested on hardware) | hardware test; iPad-friendly web console; mDNS |
| U10 | Back up an app with its data and move it to another ZPOD or card | missing | none (`zpod-app` has no export/backup) | `zpod-app export`, `backup`, `restore`; web console download |
| U11 | Update the system and keep all apps, data and music | missing | p3 is a stub, "updates mean re-flashing" (BUILDROOT.md gaps); reflashing `sdcard.img` also overwrites p4 | RAUC/tryboot A/B (HANDOFF job 1) |
| U12 | APRS chat from the iPad over the hotspot (DigiPi) | missing | no audio in, `dr_mode=peripheral`, no service apps, no web console | USB host, DigiRig, Direwolf service bundle, web console |

**Totals: 1 works, 6 partial, 5 missing.** The missing items cluster in four
places: system A/B updates (U11), an on-device app manager with data
handling (U2, U3, U10), audio for apps (U6), and the radio stack (U12).

## 9. Milestones (ordered)

Each is shippable alone; none needs a full reflash after A2.

| # | Milestone | Content | Closes |
|---|---|---|---|
| A1 | **Dev loop quick wins** (core binaries can be pushed over ssh to `/data` test paths; the image still needs one rebuild to carry them) | fix `zpod-app-push.sh` user/root/sudo; `zpod-app remove --purge`; per-app logs; launcher control socket + `zpod-app launch/stop/restart/logs`; `scripts/zpod-dev`; per-app build in `build-arm.sh` | U1, U2, U4 |
| A2 | **A/B system updates** (HANDOFF job 1) | RAUC or plain tryboot A/B into p2/p3, mark-good from the launcher, `/data` untouched; signed with the key planned for apps | U11 |
| A3 | **App manager + bundle v1** | Tools > Apps: details, remove (+data), reorder/hide, install from `/data/inbox`; `format = 1`, `kind`, `[requires]`, `[data]`, `export`/`backup`/`restore`, ed25519 signing with `dev-only` policy | U3, U10 |
| A4 | **Audio for apps + Rust buttons** | Rust buttons daemon (no Python at boot); `AudioSink` on ALSA; exclusive-audio contract with MPD; reference **generative music** app (`gen-drone`: presets in data dir, buttons map to parameters, Play = start/stop, optional render-to-WAV into `/data/media/music`) | U6 |
| A5 | **Generative art** | frame-timing helper in `zpod-ui`, fixed-point noise utilities, reference **generative art** app (`gen-flow`: seeds, presets, snapshot to `/data/media/pictures`), screensaver hook in the launcher | U7 |
| A6 | **Portability** | `zpod-hal` builds on macOS; windowed + audio simulator; `zpod-sdk` crate + `zpod-app new` templates; `zpod_pi02w_defconfig`; multi-variant bundles | U5 |
| A7 | **Hotspot web console** (HANDOFF job 3) + mDNS | iPad pages: status, Wi-Fi setup (no secrets echoed), music upload/download, app upload/install/export/backup, per-app logs; `zpod.local` | U9, helps U1/U10 |
| A8 | **USB landing page + host mode** (HANDOFF job 4) | interrogate plugged devices; runtime gadget/host switch with dev loop over Wi-Fi while in host mode; Bash Bunny class later | prerequisite for U12 |
| A9 | **Radio set (DigiPi)** | `kind = "service"` runtime; Direwolf APRS (digipeater, IGate, webchat) on Zero W first; Pat/ARDOP, FT8/JS8Call, FLDigi, SSTV, AX.25 BBS, BT KISS TNC, hamlib on Zero 2 W | U12 |

## 10. Next three steps

1. **A1 dev loop**: launcher control socket, per-app logs, `zpod-app
   launch/restart/logs/remove --purge`, fixed push script, `scripts/zpod-dev`.
   Small, all Rust, and it makes every later app cheap to iterate.
2. **A2 A/B updates**, so test3 is the last reflash (already HANDOFF job 1);
   do the signing key once and reuse it for A3.
3. **A4 audio contract + first generative music app** together with the Rust
   buttons daemon: it removes Python from the image and proves the
   `AudioSink` app API that the art, music and radio classes all depend on.
