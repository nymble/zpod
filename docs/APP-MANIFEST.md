# ZPOD app manifest (`manifest.toml`), schema 1

Status: implemented in core 0.10.0 (`apps/crates/zpod-apps/src/manifest.rs`,
`tree.rs`; launcher `apps/crates/launcher/src/menu.rs`). Container format,
install layout and signing plans: [APP-BUNDLE.md](./APP-BUNDLE.md).

Every app ships a `manifest.toml` at the top of its `.zpa`. Schema 1 adds
three things to the test3 format (schema 0):

* **`[menu]`**: where the app lives in the launcher's menu tree.
* **`[[handles]]`**: other ways to reach it (a USB device, a serial
  protocol, a file type, an input device).
* **`[requires]`**: what it needs (launcher API, USB host mode, Wi-Fi,
  inputs).

## 1. The menu tree

```
Home (ZPOD)
|- Music                     app, path ""            order 10
|- Generative >              built-in folder         order 20
|  `- Mouse Music            path "Generative"
|- Games >                   built-in folder         order 30
|  `- Tetris                 path "Games"
|- Clock                     app, path ""            order 40
|- Tools >                   always shown (built-in items + apps)
|  |- Wi-Fi 10, Bluetooth 20, USB 30, Battery 40, File transfer 50
|  |- Radio > 60             built-in folder
|  |  `- GPS                 path "Tools/Radio"
|  `- Updates 80
|- Settings >                always shown (built-in items, then apps)
`- Power >                   always shown, no apps
```

Built-in folders (`zpod_apps::tree::FOLDERS`):

| Path | Title | Order in parent | For |
|---|---|---|---|
| `""` | ZPOD (Home) | - | the everyday apps: Music, Clock |
| `Generative` | Generative | 20 | generative music and art (Mouse Music; later art apps, `Generative/Art`) |
| `Games` | Games | 30 | games |
| `Tools` | Tools | 900 (last but Settings, Power) | system tools; apps sort among Wi-Fi 10 ... Updates 80 |
| `Tools/Radio` | Radio | 60 in Tools | GPS now; SDR, APRS / DigiPi later |
| `Settings` | Settings | 910 | settings-like apps, after the built-in items |

Rules:

* A folder that has no app in it (or below it) is not shown. Tools,
  Settings and Power are always on Home.
* An app may create sub-folders **below** a built-in folder
  (`Generative/Art`, `Tools/Network`), at most 3 levels below Home, each
  name 1..20 characters of `A-Z a-z 0-9 space & + -`. It may **not** add a
  new top-level folder: Home stays short. Paths are case-insensitive;
  built-in folders keep their spelling, and the first app to create a
  sub-folder sets its spelling.
* Entries in a folder sort by `order` (lower first), then label. Built-in
  items win a tie. A folder an app creates has order 100.
* Navigation is the usual one (docs/ui/BUTTONS-AND-MENUS.md): Up/Down move,
  Right/OK/Circle open, Left/X back one level, Home to the top menu (keeps
  the selection there). Home and folder pages use 40 px rows with icons;
  Tools and Settings use 30 px text rows.
* Folder icons: `apps/assets/icons/folder-{games,generative,radio,folder}.png`
  (drawn by `apps/tools/icons/folder-icons.py`); any other folder uses
  `folder`.

`zpod-app tree` prints the tree the launcher shows; Settings > Installed
apps lists where each app is, and why one is hidden or moved.

## 2. Fields

```toml
schema = 1                     # top level; absent = 0 (test3). Newer than 1: refused

[app]                          # unchanged from schema 0
id = "gps"                     # required. [a-z0-9][a-z0-9-]{0,31}, = directory name
name = "GPS"                   # required. 1..40 chars
version = "0.2.0"              # required. MAJOR.MINOR[.PATCH]
exec = "bin/gps"               # required. relative path inside the app
args = []                      # optional
icon = "icon.png"              # optional. 32x32 PNG with alpha (menu rows, launch card)
description = "USB GPS ..."    # optional. second line of the menu row
min_core = "0.5.0"             # required. install refused if newer than /etc/zpod/core-version
order = 60                     # optional, default 100. default for [menu] order
arch = "arm-unknown-linux-musleabihf"   # optional, informational
category = "tool"              # schema 0 placement ("app" = Home, "tool" = Tools);
                               # ignored when [menu] path is set (warning)
permissions = ["usb-host"]     # optional. "usb-host": may ask the launcher to switch the USB port

[menu]
path = "Tools/Radio"           # folder; "" or "Home" = Home. Default: Tools for
                               # category "tool", else Home
order = 10                     # position in the folder; default app.order
label = "GPS"                  # menu title; default app.name

[[menu.also]]                  # optional extra placements (same app, another folder)
path = "Generative/Music"
order = 10                     # default: the main order
label = "Mouse Music"          # default: the main label

[[handles]]                    # zero or more
kind = "usb-device"            # usb-device | serial | file | input
match = ["067b:2303"]          # string or list of strings, see below
label = "GPS receiver"         # optional, shown by a chooser

[requires]
launcher_api = 2               # minimum launcher API (default 1)
usb_host = false               # needs the USB port in host mode (the app asks first)
wifi = false                   # needs a Wi-Fi network
input = ["buttons"]            # works with: buttons, keyboard, mouse, touch
```

### `[[handles]]` kinds

| kind | `match` | Used now |
|---|---|---|
| `usb-device` | `VVVV:PPPP`, `VVVV:*` (any product of the vendor) or `class:CC`, hex | Tools > USB names the app next to a plugged-in device; `zpod-app handlers usb-device 067b:2303` |
| `serial` | protocol name, e.g. `nmea` | `zpod-app handlers serial nmea` (for a future "what is on this serial port" chooser) |
| `file` | extension without the dot, e.g. `mid` | `zpod-app handlers file mid` (file opener, later) |
| `input` | `buttons`, `keyboard`, `mouse`, `touch` | `zpod-app handlers input mouse` (input chooser, later) |

Matching is case-insensitive; several apps may handle the same thing, the
lowest `app.order` comes first. A USB ID is the bridge chip's, not the
product's: `067b:2303` matches any Prolific PL2303 serial adapter, which is
why the GPS app checks for NMEA before it claims a fix.

### Launcher API levels (`zpod_apps::manifest::LAUNCHER_API`)

| API | Launcher | Adds |
|---|---|---|
| 1 | test3 | menu, launch, Home-hold kill |
| 2 | after test3 (control protocol 2) | control socket, app socket: USB role switch for apps |
| 3 | core 0.10.0 | menu tree, `[menu]` / `[[handles]]` / `[requires]`, Tools > Updates |

An app whose `launcher_api` is above the running launcher's is not shown in
the menu (Installed apps says why) and `zpod-app install` refuses it.

## 3. Compatibility

* **Unknown keys are ignored**, with a warning at install (`zpod-app
  install` / `check`) and in `zpod-app info` ("ignored: menu.shape"). A
  newer manifest therefore installs on this core, minus what it does not
  know. Schema 0 cores (test3's `zpod-app`) are strict and refuse any new
  key: install schema-1 apps with the 0.10.0 `zpod-app` (the image, or the
  `/data` core update on test3).
* **`schema`** changes only for a breaking change; a manifest with a
  higher schema than this core reads is refused ("manifest schema 2 needs a
  newer ZPOD").
* **Schema 0 manifests** (no `[menu]`) still work: `category = "tool"` goes
  to Tools, anything else to Home, except the five bundled apps, which go
  where their schema-1 manifests put them (Music and Clock at Home, Tetris
  in Games, Mouse Music in Generative, GPS in Tools/Radio), so a test3 card
  shows the same tree before its apps are updated. The image updates the
  bundled apps at boot when its versions are newer (`zpod-app update
  --if-newer`, S00zpod-data); it never reinstalls an app you removed and
  never downgrades one pushed with `zpod-dev`.
* `[menu]` etc. without `schema = 1` work but warn.

## 4. Validation

`zpod-app install`, `update` and `check` (`zpod_apps::tree::check`):

| Error (install refused) | Warning (installed) |
|---|---|
| `menu.path` / `menu.also` not under a built-in folder, too deep, bad name | unknown keys or tables |
| the same folder twice in `also` | `[menu]` without `schema = 1` |
| `handles`: empty `match`, bad USB ID / extension / input / protocol | `category` next to `[menu] path` |
| `requires.launcher_api` above this core's | unknown `handles.kind` or `requires.input` value |
| `schema` above 1, `menu.label` empty or over 40 chars | |

The launcher never refuses an installed app for its placement: a bad path
(e.g. installed by hand) puts it at Home with a note under Installed apps.

## 5. The bundled apps

```toml
# apps/packages/gps/manifest.toml
schema = 1
[app]
id = "gps"
name = "GPS"
version = "0.2.0"
exec = "bin/gps"
icon = "icon.png"
description = "USB GPS status: fix, coordinates, satellites (ND-105C)"
min_core = "0.5.0"
order = 60
permissions = ["usb-host"]
[menu]
path = "Tools/Radio"
order = 10
[[handles]]
kind = "usb-device"
match = ["067b:2303"]
label = "GPS receiver"
[[handles]]
kind = "serial"
match = ["nmea"]
[requires]
launcher_api = 2
usb_host = true
input = ["buttons"]
```

```toml
# apps/packages/mousemusic/manifest.toml (excerpt)
schema = 1
[menu]
path = "Generative"
order = 10
[[handles]]
kind = "input"
match = ["mouse", "keyboard"]
label = "Play with the mouse"
[requires]
launcher_api = 2
input = ["buttons", "mouse", "keyboard"]
```

A generative art app, a second music app in a sub-folder, and an SDR tool:

```toml
schema = 1
[app]
id = "flowfield"
name = "Flow Field"
version = "0.1.0"
exec = "bin/flowfield"
min_core = "0.10.0"
[menu]
path = "Generative/Art"        # creates Generative > Art
```

```toml
[menu]
path = "Generative"
[[menu.also]]
path = "Generative/Music"      # listed twice: Generative and Generative > Music
```

```toml
[menu]
path = "Tools/Radio"
order = 20
[[handles]]
kind = "usb-device"
match = ["0bda:2838"]          # RTL2832U dongle
label = "SDR receiver"
[requires]
usb_host = true
```

## 6. Tests

`cargo test -p zpod-apps` (schema parsing, unknown keys, schema too new,
legacy mapping, placement, `also`, handles, launcher API) and `-p
zpod-launcher` (`menu::tests::nested_folders_enter_back_home`: tree
items, Enter/Back/Home in nested folders). Simulator screenshots:
`sh apps/tools/sim/menu/run.sh <out>` in the `zpod-br` container.
