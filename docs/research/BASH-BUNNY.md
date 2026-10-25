# Bash Bunny: research and a ZPOD "gadget app" design

**Status: future (not committed).** This is research and a design proposal, the
same class as IR and game emulation in [../plan/ROADMAP.md](../plan/ROADMAP.md).
Nothing here is built or scheduled into an image unless Paul says so. No code is
added and nothing is flashed by this document.

Scope and intent: the ZPOD is a Pi Zero W with a 320x240 ILI9340 screen,
buttons, a DAC and a battery. Hak5's Bash Bunny is a USB-attack platform with no
screen. This document studies what the Bash Bunny does, then proposes a ZPOD
**USB gadget toolkit** ("gadget app") that reuses the Pi Zero's USB-gadget
hardware (we already ship CDC-NCM, see [../USB-GADGET.md](../USB-GADGET.md)) and
does the whole thing *better* because we have a GUI. It is framed as a personal
USB toolkit for Paul's own machines and authorized testing: no stored
credentials, no attack payloads aimed at third parties.

---

## Part 1 - What the Bash Bunny is

The Bash Bunny is a USB device that boots a small Linux system and emulates
combinations of trusted USB devices (keyboard, flash drive, serial, Ethernet).
By looking like ordinary hardware it can type keystrokes, present a drive, or add
a network adapter to the host. Feedback is a single RGB LED; selection is a
3-position slide switch; there is no screen and no on-device editing.

Sources: Hak5 documentation and the official payload repository, cited inline.

### 1.1 Hardware and USB emulation modes (`ATTACKMODE`)

`ATTACKMODE` is the Bunny Script command that chooses which USB devices to
emulate. It can be called more than once in a payload (e.g. bring up Ethernet,
then switch to keyboard + serial). Up to three modes run at once; each valid
combination enumerates with its own USB VID/PID, which `VID_`/`PID_`/`MAN_`/
`PROD_`/`SN_` can spoof.

| Mode | USB class | Typical host |
|---|---|---|
| `HID` | Human Interface Device | Keyboard for keystroke injection (DuckyScript) |
| `STORAGE` / `RO_STORAGE` | USB Mass Storage (UMS) | Flash drive (read-only variant) |
| `SERIAL` | CDC-ACM | Serial console |
| `ECM_ETHERNET` | CDC-ECM | Ethernet adapter for Linux / macOS / Android |
| `RNDIS_ETHERNET` | RNDIS | Ethernet adapter for Windows (and some Linux) |
| `AUTO_ETHERNET` | ECM then RNDIS | Tries ECM, falls back to RNDIS after ~20 s (fw 1.5+; `ETHERNET_TIMEOUT_XX`) |
| `OFF` | none | Disables USB emulation until the next `ATTACKMODE` |

Example valid combination: `ATTACKMODE HID STORAGE ECM_ETHERNET`. Not every
triple is legal (e.g. `RNDIS_ETHERNET ECM_ETHERNET STORAGE SERIAL` is invalid).

- ATTACKMODE reference: <https://documentation.hak5.org/bash-bunny/writing-payloads/attackmode>
- VID/PID/MAN/PROD/SN: <https://documentation.hak5.org/bash-bunny/writing-payloads/attackmode/vid-pid-man-prod-sn>
- Command summary (readme.txt): <https://github.com/hak5/bashbunny-payloads/blob/master/docs/readme.txt>

### 1.2 Scripting: Bunny Script + DuckyScript

Payloads are a file named `payload.txt`, written in **Bash + DuckyScript**. On
boot, the payload in the switch's folder runs. Keystrokes are injected with
`QUACK` (alias `Q`), which takes either inline DuckyScript or a `.txt` file;
`HID` must be in the current `ATTACKMODE`.

- DuckyScript keywords: `REM`, `DELAY`, `STRING`, `GUI/WINDOWS`, `ALT`, `CTRL`,
  `SHIFT`, arrow keys, `ENTER`, `TAB`, `F1..F12`, etc. `DUCKY_LANG=us` sets the
  keystroke layout. Firmware 1.5 added `QUACK ALTCODE <n>` (Windows alt-codes).
- Environment variables exported into payloads: `$SWITCH_POSITION`
  (`switch1`/`switch2`/`switch3`), `$HOST_IP` (the Bunny, default
  `172.16.64.1`), `$TARGET_IP` and `$TARGET_HOSTNAME` (the host, via the Bunny's
  DHCP server), `$BB_LABEL` (mass-storage volume name).
- Payload basics: <https://documentation.hak5.org/bash-bunny/writing-payloads/payload-development-basics>
- QUACK: <https://documentation.hak5.org/bash-bunny/writing-payloads/quack>

**LED status language.** The single RGB LED is the only live feedback. `LED`
takes either `COLOR [PATTERN]` or a named *state*. Colors: `R G B` plus mixes
`Y C M W`. Patterns: `SOLID`, `SLOW/FAST/VERYFAST`, and counted blinks
`SINGLE..QUIN` (and inverted `ISINGLE..`). Named states used by convention:
`SETUP`, `FAIL`, `ATTACK`, `STAGE1..5`, `SPECIAL`, `CLEANUP`, `FINISH`. This is
the entire "display": an operator must memorise color/blink codes.

**Switch positions and arming.** The 3-position slide switch selects which
payload runs: position 1 -> `/payloads/switch1/payload.txt`, position 2 ->
`/payloads/switch2/payload.txt`. Position 3 (closest to the USB plug) is
**arming mode**: the Bunny mounts as mass storage + serial so you can edit files
and get a root console. An optional `/payloads/arming/payload.txt` can override
arming (advanced; risks locking yourself out).

**Mass-storage layout** (the flash partition the host and operator see):

```
/config.txt                  global config, sourced by every payload (e.g. DUCKY_LANG us)
/payloads/
  library/                   payloads from the Hak5 repo
    extensions/              extra Bunny Script commands/functions (auto-sourced)
  switch1/payload.txt        runs in switch position 1
  switch2/payload.txt        runs in switch position 2
  arming/payload.txt         optional override for position 3
/loot/                       logs and captured data; payloads commonly write
                             /root/udisk/loot/<name>/
/tools/                      helper binaries/debs a payload needs
```

Default credentials are documented as `root` / `hak5bunny`, hostname `bunny`,
DHCP range `172.16.64.10-12`. (We will **not** reuse those defaults.)

- Mass storage structure: <https://documentation.hak5.org/bash-bunny/getting-started/mass-storage-structure>
- readme.txt (layout + env vars + LED table): <https://github.com/hak5/bashbunny-payloads/blob/master/docs/readme.txt>

### 1.3 Extensions, `RUN`, `GET`, `REQUIRETOOL`

Extensions in `/payloads/library/extensions` are sourced automatically and add
functions on top of DuckyScript:

- `GET <VAR>` exports a system variable (`GET TARGET_IP`, `GET HOST_IP`,
  `GET TARGET_HOSTNAME`, `GET SWITCH_POSITION`).
- `RUN <OS> <command>` is a multi-OS "open a run box and type a command"
  shortcut: `RUN WIN notepad.exe` (Win+R), `RUN WIN_ADMIN ...` (Ctrl+Shift+Enter
  for an elevated prompt), `RUN OSX ...` (Spotlight), `RUN UNITY`/`RUN LINUX`
  (Alt+F2). It is implemented purely as `QUACK` keystrokes.
- `REQUIRETOOL <name>` fails the payload (`LED FAIL`) if the tool is missing
  from `/tools`. `DUCKY_LANG <cc>` sets the HID language.
- Extensions reference: <https://documentation.hak5.org/bash-bunny/writing-payloads/extensions>
- `run.sh` source: <https://github.com/hak5/bashbunny-payloads/blob/master/payloads/extensions/run.sh>

### 1.4 Payload library and categories

The official repo `hak5/bashbunny-payloads` organises community payloads under
`payloads/library/<category>/<name>/` with `payload.txt`, a `readme.md`, and an
optional `install.sh`. Categories include: `credentials`, `exfiltration`,
`recon`, `execution`, `remote_access`, `phishing`, `prank`, `general`, `mobile`,
`Incident_Response`, and `poc`.

Representative official payloads (summarised, not copied):

- **Nmapper** (`recon`): brings up Ethernet, waits for a `TARGET_IP` lease, runs
  `nmap -O --fuzzy` to fingerprint the host OS, and writes sequential logs to
  `/root/udisk/loot/nmap`. LED walks SETUP -> (FAIL if no lease) -> ATTACK ->
  CLEANUP -> FINISH. Good model of the "get IP, run a tool, save loot" shape.
  <https://github.com/hak5/bashbunny-payloads/blob/master/payloads/library/recon/nmapper/payload.txt>
- **SMB Exfiltrator** (`exfiltration`): HID stage launches a hidden PowerShell
  stager, Ethernet stage runs an impacket SMB server, files copy over SMB to the
  Bunny's loot folder, then CLEANUP/FINISH. Illustrates multi-stage HID +
  Ethernet, `REQUIRETOOL`, and a "complete" sentinel file.
- Contributing rules forbid real URLs, keys, or passphrases in shared payloads
  (use `example.com` placeholders) and require config vars at the top of the
  file and documented LED states.
- Library index: <https://github.com/hak5/bashbunny-payloads/tree/master/payloads/library>
- Best practices: <https://documentation.hak5.org/bash-bunny/writing-payloads/contributing-best-practices>

**Sharing.** Payloads are shared through the GitHub repo and showcased/awarded on
**PayloadHub** (<https://payloadhub.com/>). Hak5 also offers **PayloadStudio**, a
browser-based editor. There is no on-device authoring beyond editing the text
files in arming mode.

### 1.5 Limitations of the Bash Bunny (the opening for ZPOD)

| Limitation | Effect on the operator |
|---|---|
| **No screen** | Status is one RGB LED; you memorise color/blink codes. No names, no progress, no error text. |
| **Switch-only selection** | At most two armed payloads (switch 1 / 2) plus arming; to run a third you re-arm and copy files. |
| **No on-device editing** | Editing means arming mode + a host text editor, or PayloadStudio in a browser. Nothing on the device itself. |
| **No confirm / no dry-run** | A payload fires the instant the switch selects it on boot. Destructive actions have no on-device guard. |
| **Loot is "find the folder"** | Results land in `/loot`; you mount the drive on a host to read them. No on-device summary. |
| **ECM vs RNDIS vs NCM quirks** | ECM = Linux/macOS/Android, RNDIS = Windows; macOS's ECM driver is buggy (we hit the AppleUserECM 161-frame bug, see USB-GADGET.md). Picking the wrong mode silently fails, and the LED cannot tell you which. |

Every one of these is something a 320x240 screen plus buttons fixes directly.

---

## Part 2 - ZPOD "gadget app" design

The idea: a ZPOD app, `gadget` (working name "ZPOD Gadget"), that plays the same
role as the Music app but for USB gadget *scripts* instead of songs. Paul's
framing - "like the music: an ability to load new scripts, a basic set included,
app-specific loading of data" - maps cleanly onto the existing ZPOD app and
file-transfer model.

### 2.1 How this reuses what ZPOD already has

- **USB gadget**: M4 already ships a configfs/libcomposite CDC-NCM gadget
  (`zpod-usb-gadget.sh`, `/boot/firmware/zpod-usb.txt` selecting `ncm`/`ecm`/
  `g_ether`), see [../USB-GADGET.md](../USB-GADGET.md). The gadget app would add
  *more* functions to the same configfs gadget (HID, mass storage, ACM serial)
  rather than inventing a new stack. **NCM is already in M4; everything else here
  is future.**
- **App model**: an app installs under `/opt/zpod/apps/<id>/` with a
  `manifest.toml`, runs as the unprivileged `zpod` user, owns the LCD and reads
  buttons from evdev - exactly the Music/Clock/Tetris contract in
  [../APPS.md](../APPS.md).
- **File transfer**: user data arrives over the same links as music - rsync/scp
  over USB (`10.12.194.1`) or Wi-Fi (`zpod.local`), and the `zpod-drop` HTTP
  server - described in [../FILE-TRANSFER.md](../FILE-TRANSFER.md). We extend
  `zpod-drop` with a scripts/loot area instead of writing a new uploader.

### 2.2 Data layout (parallel to `/srv/zpod/media/music`)

Installed **app code** lives under `/opt/zpod/apps/gadget/` (read-only, like any
app). User-loadable **scripts and data** live under `/srv/zpod/`, parallel to
`/srv/zpod/media/music`, matching Paul's suggested `payloads/ loot/ config/`:

```
/srv/zpod/gadget/                 (owned by zpod; data, not app code)
  scripts/                        user-loadable gadget scripts (the "payloads")
    <id>/
      manifest.toml               name, description, modes, target OS, tags, safety class
      script.sh                   the Bunny-Script-like body (bash + our QUACK shim)
      keys.txt                    optional DuckyScript keystroke file
      readme.md                   optional human notes
  loot/                           per-script output, read back on screen
    <id>/<timestamp>/...
  config/                         app + per-script config (no secrets in git)
    gadget.toml                   defaults: safe mode, confirm policy, NCM/ECM choice
  logs/                           on-device run history (what ran, when, result)
```

Notes:
- This mirrors the Bunny's `/payloads` + `/loot` + `config.txt`, but keyed by a
  readable `<id>` with a manifest instead of `switch1/switch2`.
- Paul's literal suggestion was `/srv/zpod/apps/bunny/{payloads,loot,config}/`.
  That works too; the only reason to prefer `/srv/zpod/gadget/` is to avoid
  confusion with `/opt/zpod/apps/` (app code) - open question 1 below.
- `.gitignore` already keeps images/artifacts out; `loot/`, `logs/` and
  `config/` would be runtime-only and never committed.

**Script manifest** (same spirit as the app `manifest.toml` in APPS.md, so one
parser style across the project):

```toml
[script]
id = "usb-file-drop"
name = "USB File Drop"
description = "Present a read-only thumb drive of chosen files to the host"
modes = ["STORAGE"]            # subset of HID/STORAGE/SERIAL/ECM/RNDIS/NCM
target_os = ["any"]           # any | windows | macos | linux
tags = ["storage", "safe"]
safety = "safe"               # safe | dual-use | destructive
confirm = false               # force a confirm screen before running
author = "paul"
```

### 2.3 Transferring new scripts (same as music/apps)

No new transport. Reuse [../FILE-TRANSFER.md](../FILE-TRANSFER.md):

1. **rsync/scp over SSH** into `/srv/zpod/gadget/scripts/` (USB `10.12.194.1`,
   or Wi-Fi `zpod.local`), the same pattern as `zpod-push-music.sh`. A
   `scripts/zpod-push-gadget.sh` helper would mirror it.
2. **`zpod-drop` HTTP server**: add an accepted path prefix for
   `scripts/<id>/...` (today it only accepts audio/cover/playlist extensions
   under `/music`). Same safety rules already in drop apply: reject `..`, hidden
   and absolute paths, atomic `.part` writes, per-file cap, free-space floor,
   USB = no auth, Wi-Fi = token required.
3. **Packaged scripts** could ride the existing `.zpa` / `zpod-app` path if we
   want signed bundles later (APPS.md already flags package signing as a
   follow-up).

Loot and logs flow back out the same way (read over the drop server's `GET`, or
rsync the `loot/` tree to the Mac).

### 2.4 What we do BETTER because we have a GUI

This is the heart of the proposal. The Bunny's limitations in 1.5 become ZPOD
features:

1. **On-device browser / select / arm.** The app lists scripts by readable
   **name + description + tags** (from the manifest), not switch positions. D-pad
   to scroll, OK to open, a per-script detail screen shows modes, target OS and
   safety class. You can hold any number of scripts, not two.
2. **Live status instead of blink codes.** The status bar shows the current
   `ATTACKMODE` (e.g. "NCM + Serial"), detected/target OS, a progress line and
   real **error text** ("no DHCP lease", "HID not enumerated"), replacing SETUP/
   FAIL/ATTACK LED states. We can map the Bunny's state names to on-screen
   labels so existing scripts stay legible.
3. **Confirm-before-run and dry-run / simulate.** Any script marked
   `safety = "dual-use"` or `"destructive"`, or with `confirm = true`, shows a
   confirm dialog (OK/X, like the Reboot/Shutdown dialog in APPS.md). A
   **dry-run** mode renders the keystrokes/commands a script *would* send and the
   modes it *would* enable, without enumerating anything - impossible on an
   LED-only device.
4. **On-device metadata edit + last-loot summary.** Edit name, tags and target
   OS in a simple form; after a run, show a **loot summary** screen (files
   captured, bytes, exit status) read from `loot/<id>/<timestamp>/`. No mounting
   a drive on a host to find out what happened.
5. **Playlists / sequences.** Like music playlists: an ordered list of scripts
   that run in sequence with on-screen progress and per-step confirm. Natural fit
   for "bring up serial, run a macro, drop a file".
6. **Safe defaults.** The default mode is **storage-only "thumb drive"**
   (`RO_STORAGE`), which does nothing surprising to a host. `HID` and the
   Ethernet modes require an **explicit arm** gesture in the UI each session;
   they never fire just because the app opened. This replaces "payload runs the
   instant the switch picks it on boot".
7. **Logging and on-screen history.** `logs/` records what ran, when, which modes
   and the result; a History screen shows it on the device. The Bunny has no
   equivalent.
8. **Mode picker that knows the host-OS quirks.** Because we learned the macOS
   ECM bug the hard way (USB-GADGET.md), the UI can default to **NCM**, warn
   about ECM-on-macOS and RNDIS-on-Windows, and show which the host actually
   bound - turning the silent ECM/RNDIS/NCM footgun into a visible choice.

### 2.5 Proposed basic (non-malicious) included script set

A starter set that is genuinely useful for a personal toolkit and clearly
labelled. Nothing here captures third-party credentials or attacks a host; the
dual-use items are for Paul's own machines / authorized testing only.

| id | Name | Modes | Safety | What it does |
|---|---|---|---|---|
| `usb-file-drop` | USB File Drop | `RO_STORAGE` | safe | Present a read-only thumb drive of chosen files (e.g. drivers, installers, your own music) to a host. The default mode. |
| `ncm-netgadget` | Network Gadget | `NCM` (ECM/RNDIS opt) | safe | Bring up the USB Ethernet link that M4 already ships; show the link the host bound and the lease. Pure status/convenience. |
| `serial-console` | Serial Console | `SERIAL` | safe | Expose a CDC-ACM console (`getty@ttyGS0`) so a host can log in when Wi-Fi is down - the follow-up already noted in FILE-TRANSFER.md. |
| `my-macros` | My Keyboard Macros | `HID` | dual-use | Type *your own* saved snippets/boilerplate into *your own* machine (needs explicit arm + confirm). Owner-only text; no stored passwords. |
| `net-identity` | Net Identity | `NCM` | safe | Show `HOST_IP`/`TARGET_IP`/hostname of whatever it's plugged into; a labelled read-only helper, no scanning. |
| `host-info` | Host Info (dry-run first) | `HID`+`SERIAL` | dual-use | On *your own* machine, run a read-only "what OS/version am I" command and save the output to `loot/` - always offered as dry-run first. |

Deliberately **excluded** from the included set: anything that grabs saved
browser/Wi-Fi/OS credentials, exfiltrates a third party's files, or installs
persistence. Those are what the Bunny's `credentials`/`exfiltration` categories
do; this doc is a design for a personal gadget toolkit and intentionally stops
at owner-only, labelled, dual-use at most.

### 2.6 Feature -> milestone map

Marked **future (not committed)** unless noted. Only the CDC-NCM gadget exists
today.

| Capability | State | Milestone |
|---|---|---|
| CDC-NCM USB gadget | **built (M4)** - already shipping | M4.5 |
| `RO_STORAGE` / mass-storage gadget function | future | M-gadget.1 |
| CDC-ACM serial console (`getty@ttyGS0`) | future (follow-up already noted in FILE-TRANSFER.md) | M-gadget.1 |
| HID gadget + QUACK/DuckyScript shim | future | M-gadget.2 |
| Gadget app UI: browser, status bar, confirm, dry-run | future | M-gadget.2 |
| `/srv/zpod/gadget/` layout + manifest parser | future | M-gadget.1 |
| `zpod-drop`/`zpod-push-gadget.sh` script transfer | future (extends existing transfer) | M-gadget.1 |
| Loot summary, on-device history/logging | future | M-gadget.2 |
| Script playlists/sequences | future | M-gadget.3 |
| Signed script packages (`.zpa`-style) | future (ties into APPS.md package-signing follow-up) | M-gadget.3 |

---

## Open questions for Paul

1. **Path name**: `/srv/zpod/gadget/{scripts,loot,config,logs}/` (proposed, to
   keep it distinct from `/opt/zpod/apps/` app code) versus your literal
   suggestion `/srv/zpod/apps/bunny/{payloads,loot,config}/`? Same shape either
   way - just the name.
2. **App / id name**: call it `gadget` ("ZPOD Gadget"), or keep the "bunny"
   name? "gadget" reads as first-party and avoids the Hak5 trademark.
3. **How far on HID**: are you happy keeping HID strictly owner-only
   (type-your-own-macros, confirm + explicit arm), or do you want a broader
   DuckyScript-compatibility layer so existing community `.txt` scripts run as-is
   on the ZPOD?
4. **Commit level**: confirm this stays future-uncommitted (same as IR and game
   emulation) until you decide to pull a gadget milestone into an image.
5. **Serial console**: do you want the CDC-ACM `getty@ttyGS0` follow-up (from
   FILE-TRANSFER.md) promoted as the first concrete gadget step, since it's the
   lowest-risk and most useful on its own?
