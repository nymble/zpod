# ZPOD app bundle (`.zpa`) specification

Status: **v0 = what the code does today** (core 0.9.0, `apps/crates/zpod-apps`,
`apps/crates/launcher`); **v1 = proposed** (2026-11-03, not implemented).

> **Update (core 0.10.0, 2026-11-05):** manifest **schema 1** is implemented
> and specified in [APP-MANIFEST.md](./APP-MANIFEST.md): top-level
> `schema = 1` (instead of the proposed `format`), `[menu]` (folder path,
> order, label, extra placements) replacing `category`, `[[handles]]` (USB
> VID:PID, serial protocol, file type, input) and `[requires]`
> (`launcher_api`, `usb_host`, `wifi`, `input`). Unknown keys are now
> **ignored with a warning**, not errors, so newer manifests install on
> older cores from 0.10.0 on. The container (section 2), install layout and
> the other [proposed] items below are unchanged.
Every proposed item is marked **[proposed]**. Anything not marked is
implemented and has a code reference.

Companion docs: [APPS.md](./APPS.md) (launcher, runtime), [ROADMAP-APPS.md](./ROADMAP-APPS.md)
(app classes, lifecycle, dev loop, milestones).

## 1. Verdict on the current format

The format is **well implemented but under-specified**. The code is strict and
safe (atomic install, path and ELF checks, `deny_unknown_fields`), and APPS.md
describes it, but there was no standalone spec, and these are not defined at all:

| Area | Today | Gap |
|---|---|---|
| Format version | none in the archive or manifest | a new key breaks old cores (`deny_unknown_fields` rejects it) with no way to say "this is format 1" |
| Manifest | `[app]` only: id, name, version, exec, args, icon, description, min_core, order, arch, category (`app`/`tool`, added after test3) (`manifest.rs`) | no app kind, no resource needs, no data policy, no author/licence/homepage |
| Arch | `arch` is informational; installer checks every ELF is 32-bit ARM, ARMv6-compatible (`elf.rs`) | no multi-arch bundles (Zero W armv6 vs Zero 2 W armv7/aarch64, desktop sim) |
| Permissions / resources | none; every app runs as `zpod` with groups spi, gpio, i2c, input, audio, video (`launcher/src/proc.rs`, APPS.md) | any app can drive SPI, GPIO, I2C; no declared need for audio, network, USB host, radio |
| Data dir | convention: `$ZPOD_DATA_DIR/<id>` (`zpod-hal/src/lib.rs` `data_dir`), `/data/zpod/appdata` on Buildroot | not in the manifest; `zpod-app remove` leaves it behind; no backup/export; no migration on upgrade |
| Install / update / remove | `zpod-app install/update/remove/check/pack` (`zpod-apps/src/main.rs`, `package.rs`) | remove has no "and data" option; no on-device UI (Installed apps is read-only, `launcher/src/main.rs` Page::Apps); no `export` of an installed app |
| Signing | none (APPS.md "Not done yet") | anyone with sudo installs anything |
| Integrity | gzip CRC only | no per-file hashes, no bundle digest |
| Dev mode | none | no unsigned/dev install marker, no launch/restart/log from the Mac |
| Background services | not supported: one foreground app at a time, launcher `exec`s and waits | radio/DigiPi class needs daemons (Direwolf, Pat, web console) |

## 2. Container (v0, implemented)

* File name: `<id>-<version>.zpa` (`package::pack`).
* gzip-compressed POSIX/GNU tar. `manifest.toml` at the top level, written as
  the **first** entry by `pack` (so `inspect` reads it without unpacking all).
* Members: regular files and directories only. **Rejected:** absolute paths,
  `..`, symlinks, hard links, device nodes, FIFOs (`package::clean`, `extract`).
* Limits: at most 10,000 entries and 256 MiB unpacked (`MAX_ENTRIES`, `MAX_TOTAL_BYTES`).
* Modes are normalised to 0644, or 0755 for executables and `exec`; owner root;
  mtime = `SOURCE_DATE_EPOCH` (default 0): `pack` is reproducible.
* Manifest read limit: 64 KiB.

**[proposed v1]** Add `META/` reserved directory: `META/FILES.sha256`
(sha256 of every member, sorted), `META/SIGNATURE` (see 7). Members under
`META/` are not installed as app files.

## 3. Installed layout (v0)

```
$ZPOD_APPS_DIR/<id>/            default /opt/zpod/apps; on Buildroot /opt/zpod/apps -> /data/zpod/apps
  manifest.toml
  icon.png                      optional, 32x32 PNG with alpha
  bin/<exec>                    static arm-unknown-linux-musleabihf ELF, or a #! script
  ...                           read-only assets
$ZPOD_DATA_DIR/<id>/            app data (settings, saves); /data/zpod/appdata/<id> on Buildroot
```

Directory name must equal `id` or the launcher lists it as BROKEN (`zpod_apps::discover`).
Names starting with `.` are skipped (staging, `.old-*`, `.removing-*`, `.lock`).

## 4. Manifest

### 4.1 v0 fields (implemented, `manifest.rs`)

```toml
[app]
id = "tetris"            # required. [a-z0-9][a-z0-9-]{0,31}; = directory name
name = "Tetris"          # required. 1..40 chars, menu label
version = "1.0.0"        # required. MAJOR.MINOR[.PATCH], optional leading v, -pre/+build ignored for ordering
exec = "bin/tetris"      # required. relative, no "..", no ".", no "\", must be a regular file
args = []                # optional
icon = "icon.png"        # optional; missing file = warning + default icon
description = "..."      # optional
min_core = "0.5.0"       # required. install refused if > /etc/zpod/core-version
order = 30               # optional, default 100; menu sort (then name)
arch = "arm-unknown-linux-musleabihf"   # optional, informational only
```

Unknown keys and unknown tables are **errors** (`deny_unknown_fields`) up to
core 0.9.x. From core 0.10.0 (schema 1) they are ignored with a warning, and
`category` is replaced by `[menu] path`: see [APP-MANIFEST.md](./APP-MANIFEST.md).

### 4.2 v1 additions [proposed]

(Partly superseded: `schema`, `[menu]`, `[[handles]]` and a first
`[requires]` are implemented as in [APP-MANIFEST.md](./APP-MANIFEST.md);
where this section differs, APP-MANIFEST.md wins.)

Compatibility rule: a v1 manifest sets `format = 1`, and any v1-only key
requires `min_core >= 0.10.0` (the first core that would parse v1). A v0 core
(0.9.0 and older) parses strictly **before** it checks `min_core`, so it
refuses a v1 bundle with "unknown field" rather than "needs core 0.10.0". That
is safe but unfriendly; the v1 `zpod-app` therefore reads `format` and
`min_core` leniently first, then parses strictly. Unknown keys stay errors in
v1 except under an `[x-<vendor>]` table, which is ignored.

```toml
format = 1                       # [proposed] bundle format; absent = 0

[app]
# ... all v0 fields ...
kind = "app"                     # [proposed] app | tool | service | gadget
                                 #   app:     full-screen, launched from Home (v0 behaviour)
                                 #   tool:    listed under Tools, same runtime as app
                                 #   service: background daemon, started/stopped by the launcher (radio set)
                                 #   gadget:  USB-gadget payload (Bash Bunny class), opt-in
category = "art"                 # [proposed] music | art | games | radio | tools | net | other
author = "nymble"                # [proposed]
license = "MIT"                  # [proposed] SPDX id
homepage = "https://..."         # [proposed]

[requires]                       # [proposed] resources the app needs; install warns, launch checks
display = true                   # owns the LCD (default true for app/tool)
buttons = true
audio_out = "exclusive"          # none | shared | exclusive (exclusive pauses MPD while running)
audio_in = false                 # USB sound card / DigiRig (radio set)
network = "none"                 # none | lan | internet
usb_host = false                 # needs the data port in host mode (loses the USB dev link)
radio = []                       # e.g. ["wifi-scan", "bluetooth", "ptt-serial"]
i2c = false                      # direct /dev/i2c-1 (battery/RTC) - normally via zpod-hal instead
gpio = false                     # direct GPIO/SPI - only system apps
min_ram_mb = 0
cpu = "armv6"                    # armv6 (Zero W ok) | armv7 (Zero 2 W only) | any

[data]                           # [proposed]
dir = true                       # gets $ZPOD_DATA_DIR/<id> (today: always)
keep_on_remove = "ask"           # ask | keep | delete  (default ask on device, keep for zpod-app remove)
schema = 1                       # data schema version; upgrade runs migrate hook when it changes
migrate = "bin/migrate"          # optional, run once as the app user before first launch after upgrade
export = ["scores.toml", "presets/"]   # what `zpod-app backup` includes (default: whole dir)

[service]                        # [proposed] only for kind = "service"
exec = "bin/aprsd"
autostart = false
web = 8081                       # port shown on the hotspot web console
restart = "on-failure"

[[variant]]                      # [proposed] multi-target bundles
target = "arm-unknown-linux-musleabihf"      # Zero W + Zero 2 W
exec = "bin/armv6/app"
[[variant]]
target = "armv7-unknown-linux-musleabihf"    # Zero 2 W only, faster (NEON)
exec = "bin/armv7/app"
[[variant]]
target = "sim"                               # desktop simulator (macOS/Linux host)
exec = "bin/sim/app"
```

## 5. Runtime contract

v0 (implemented; launcher `main.rs` `launch`, `proc::spawn_app`, `zpod-launcher-run`):

* Run as user `zpod` (`ZPOD_APP_USER`), own process group, cwd = app dir.
* Env: `ZPOD_APP_ID`, `ZPOD_APP_DIR`, `ZPOD_CORE_VERSION`, inherited
  `ZPOD_DATA_DIR` (Buildroot: `/data/zpod/appdata`), `HOME`, `USER`, `LOGNAME`.
* stdout/stderr: the app's own log `/data/zpod/logs/<id>.log` (launcher from
  2026-11-05; rotated to `.log.1` at launch above 256 KiB; `/run/zpod/apps`
  without `/data/zpod`). Older launchers: inherited, `/run/zpod/launcher.log`.
* The launcher releases the LCD before exec and re-opens it after exit. Exit 0
  = back to Home; non-zero or signal = toast. Hold Home 3 s = SIGTERM to the
  group, SIGKILL 1.5 s later.
* Volume keys are already applied by the buttons daemon; apps only redraw.

[proposed v1]:

* Per-app log: done (above); still to do: readable from the web console.
* `ZPOD_RUN_MODE=normal|dev`, `ZPOD_TARGET=pi0w|pi02w|sim`. (`ZPOD_SOCKET`
  exists, but only for `zpod-app` and tests; apps get no socket access.)
* Audio: if `requires.audio_out = "exclusive"`, the launcher pauses MPD before
  exec and resumes after (same rule as the test tone today), and the app opens
  ALSA `default`. `shared` needs a dmix device, which does not exist yet
  (`/etc/asound.conf` is a plain `plug` to `hw:`; MPD opens `hw:` directly).
* Apps that did not declare `gpio`/`i2c` run without the spi/gpio/i2c groups.

## 6. Lifecycle commands

| Command | v0 | v1 [proposed] |
|---|---|---|
| `zpod-app pack <dir> [out]` | yes | + writes `META/FILES.sha256`; `--sign <key>` |
| `zpod-app check <zpa>` | yes | + shows `requires`, signature state |
| `zpod-app install/update [--force] <zpa>` | yes (atomic, staged, lock) | + signature policy, `migrate` hook, `--dev` |
| `zpod-app remove [--purge] <id>` | yes; data kept; **`--purge` (2026-11-05)** also deletes `$ZPOD_DATA_DIR/<id>` and its logs | |
| `zpod-app list`, `info` | yes | + data size, signer |
| `zpod-app export <id> [out]` | no | rebuild a `.zpa` from an installed app (move/transfer) |
| `zpod-app backup <id\|--all> [out]` / `restore <file>` | no | `<id>-data-<date>.tar.gz` of the data dir, with the app version in it |
| `zpod-app launch/stop/restart <id>`, `status` | **yes (2026-11-05)**, via `/run/zpod/launcher.sock` ([DEV-LOOP.md](./DEV-LOOP.md)) | |
| `zpod-app logs [-f] [-n N] <id>` | **yes (2026-11-05)**, `/data/zpod/logs/<id>.log` | |
| `scripts/zpod-dev <id>` (Mac side) | **yes (2026-11-05)**: build, pack, push over USB, `install --force`, `restart`, `logs -f` | `--dev` install + badge |
| `zpod-app new <id> --template art\|music\|tool` | no | starter crate using zpod-hal + zpod-ui |

## 7. Signing [proposed]

* Key type ed25519, minisign-compatible format so the Mac needs only `minisign`.
* `META/SIGNATURE` signs `META/FILES.sha256` plus `manifest.toml`'s sha256.
* Trust store `/etc/zpod/trusted-keys/*.pub` (image) plus
  `/data/etc/zpod/trusted-keys/` (user-added).
* Policy `/data/etc/zpod/apps.conf`: `unsigned = allow | dev-only | deny`
  (default `dev-only`: unsigned bundles install only with `--dev`, and show a
  "DEV" badge in the launcher).
* The same key pair can sign RAUC system bundles, so one trust root covers
  system and apps.

## 8. Conformance tests

v0 tests live in `zpod-apps/src/{manifest,package}.rs` (parse, reject bad
manifests, traversal, symlinks, version order, pack/install/update/remove,
min_core). v1 should add: `format` pre-scan on a v0-style parser, variant
selection per target, `--purge`, export round-trip (export -> install on a
clean dir gives the same file hashes), backup/restore, signature accept/reject.
