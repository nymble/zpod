# App dev loop: edit on the Mac, see it on the ZPOD, no reflash

Step 1 of [ROADMAP-APPS.md](./ROADMAP-APPS.md) section 10 (A1). Built and
tested on 2026-11-05 on the Buildroot test3 card
(`v0.9.0-br-m6-fastboot-test3`) over the USB cable.

## Use it

One time per card (and again after a reboot of a test3 card, which
`zpod-dev` does by itself; see "What lives where"):

```zsh
cd ~/cb/projects/zpod
scripts/zpod-dev --core          # build + push zpod-launcher and zpod-app to /data, switch to them
```

Then, for every change:

```zsh
scripts/zpod-dev clock           # build, pack, push, install, restart on the screen, follow its log
```

Ctrl-C stops following the log; the app keeps running. Other commands:

```zsh
scripts/zpod-dev --logs clock    # only follow the log
scripts/zpod-dev --no-logs clock # build + push + restart, then return
scripts/zpod-dev --build-only clock   # -> apps/dist/dev/clock-<version>.zpa
scripts/zpod-app-push.sh [--restart] apps/dist/dev/clock-1.1.0.zpa   # push any .zpa
```

On the device (`ssh root@10.12.194.1`; `/usr/local/bin` is not on dropbear's
PATH, so use the full path or `export PATH=/usr/local/bin:$PATH`):

```sh
zpod-app status                  # running clock (pid 1234, 12 s)  |  idle (home screen)
zpod-app launch tetris           # stops the running app first
zpod-app restart [clock]         # restart the running app (or start clock)
zpod-app stop                    # back to the home screen
zpod-app logs [-f] [-n 50] clock # /data/zpod/logs/clock.log
zpod-app remove --purge devtest  # uninstall and delete /data/zpod/appdata/devtest and its logs
```

Options for both scripts: `-H host` / `--host=host` (see "Over Wi-Fi"),
`-u user` (default `$ZPOD_USER` or `root`). With a non-root user (Pi OS:
`-u pi`) every device command runs with `sudo`, which may ask for the
password in the terminal; nothing is stored.

## Over Wi-Fi (no cable; needed in USB host mode)

The ZPOD must be a Wi-Fi client (Tools > Wi-Fi > Mode: Client). Save its
Wi-Fi address once, asked over the current link (USB):

```zsh
scripts/zpod-dev --save-host          # -> ~/.zpod-host (or: --save-host 192.168.1.x)
```

```zsh
scripts/zpod-dev --which-host         # what the scripts will use now
```

Without `-H` / `ZPOD_HOST` the scripts try, in order, the saved address,
the USB address `10.12.194.1`, `zpod.local` and `zpod` (router DNS; the
Buildroot image has no mDNS), and use the first one whose SSH port
answers (1 s each); they print `zpod: using <addr>` when it is not USB.
`-H addr` or `ZPOD_HOST=addr` skips the probing. SSH to the device is
root with a key only (see [USB-HOST.md](./USB-HOST.md#security)).

Measured 2026-11-05 on test3 at home (Mac mini M4, 2.4 GHz, Zero W
BCM43430), same commands as above:

| | Wi-Fi | USB |
|---|---|---|
| ping (avg) | 12.8 ms | 1.3 ms |
| first SSH connection / reused | 1.1 s / 0.05 s | 0.3 s / 0.04 s |
| 4 MiB over SSH | 5.8 s (~0.7 MB/s) | 0.75 s (~5.3 MB/s) |
| `zpod-dev --no-logs clock`, nothing changed | 1.6-1.9 s (push+restart 1.3-1.5 s) | 0.8-1.5 s (push+restart 0.5-1.2 s) |
| `zpod-dev --no-logs clock`, one-line change | **3.8-5.8 s** | 3.7-4.3 s |
| `zpod-dev --core` (launcher + zpod-app) | ~15 s | ~10 s |
| `zpod-app logs -n 3 clock` | 0.08 s (reused connection) | |

After a reboot, the Wi-Fi client and SSH come back on their own (test3:
SSH port on Wi-Fi answered 29 s after `reboot`); the `/data` core overlay
on test3 does not, so run `scripts/zpod-dev --core` again.

## Measured (2026-11-05, Mac mini M4, colima `zpod-br`, Zero W over USB)

| Change | Build | Push + install + restart | Total (command start to app running) |
|---|---|---|---|
| clock title text (`ui.rs`) | 3.3 s cargo (3.8 s step) | 0.5 s | **4.3 s** |
| revert of that change | 2.9 s cargo (3.2 s step) | 0.5 s | **3.7 s** |
| nothing changed | 0.3 s | 0.5 s | ~0.8 s |
| first run after `--core` (relinks; builds the host `zpod-app` once) | 6.4 s | 0.7 s | 7.1 s |
| after a reboot of test3 (re-applies the `/data` overlay first) | 3.6 s | 2.9 s | 6.5 s |
| `--core` (launcher + zpod-app) | ~7 s | ~2.5 s (incl. launcher restart) | ~10 s |

The app's first frame follows within about a second (the launch card shows
until then). Target was about 30 s.

## How it works

```
Mac  scripts/zpod-dev <id>
  1  docker exec zpod-br: rsync apps/ -> /br/apps-dev (never mixes in a macOS target/)
     apps/tools/build-app.sh <id>: cargo zigbuild -p zpod-<id> --bin <bin> (ARMv6 musl),
     Tag_CPU_arch check, stage apps/packages/<id>/ + binary, zpod-app pack
     -> apps/dist/dev/<id>-<version>.zpa (via the repo mount)
  2  scripts/zpod-app-push.sh --restart: one ssh round trip (shared ControlMaster
     connection): tar stream -> /tmp, zpod-app install --force, zpod-app restart <id>
  3  ssh -t: zpod-app logs -f <id>
ZPOD
  zpod-app restart <id> -> /run/zpod/launcher.sock -> launcher: SIGTERM the app's
  process group (SIGKILL after 1.5 s), rescan, launch the new binary as user zpod,
  stdout/stderr -> /data/zpod/logs/<id>.log
```

* **Control socket** `/run/zpod/launcher.sock` (root, 0600): one request line,
  one reply line. `launch <id>`, `restart [<id>]`, `stop [<id>]`, `status`,
  `version`. Protocol and client: `apps/crates/zpod-apps/src/ctl.rs`; server:
  `apps/crates/launcher/src/ctl.rs`. The launcher stays the only LCD owner;
  nothing on the Mac side touches the panel.
* **Per-app logs** `/data/zpod/logs/<id>.log` (`/run/zpod/apps` when there is
  no `/data/zpod`). Lines `--- <time> start <id> <version> pid N` and
  `--- <time> end <id>: <how> after N s` frame each run. A log over 256 KiB is
  moved to `<id>.log.1` at the next launch. `zpod-app logs -f` follows across
  rotation and exits when its reader (the ssh session) goes away.
* **`install --force`** reinstalls the same version, so the dev loop never
  has to bump versions. Install now also makes every directory 0755: under a
  root ssh session (umask 0077) an installed app was unreadable for user zpod
  (`Permission denied` at launch); that was a bug in the image's `zpod-app`.
* **`remove --purge`** stops the app if it is on the screen, uninstalls it
  and deletes `$ZPOD_DATA_DIR/<id>` (`/data/zpod/appdata/<id>`) and its logs.
  Plain `remove` keeps the data and says where it is.

## What lives where (test3 and later images)

The launcher and `zpod-app` are on the read-only squashfs root
(`/usr/lib/zpod/bin`), and test3 has no hook that runs anything from `/data`
at boot. So:

| Piece | test3 card (now) | Next image (in this commit) |
|---|---|---|
| apps (`.zpa`) | `/data/zpod/apps`, installed by `zpod-app`: no reflash, persistent | same |
| launcher + `zpod-app` with the dev features | `/data/zpod/core/bin/`, bind-mounted over `/usr/lib/zpod/bin/<name>` by `/data/zpod/core/zpod-core-overlay apply --restart` (`scripts/zpod-dev --core`). **Lost at reboot**: the image's launcher comes back (no control socket); the next `scripts/zpod-dev <id>` sees that and re-applies the overlay from `/data` in about 3 s | `zpod-data-early` runs `zpod-core-overlay boot` before the launcher starts: the update loads at every boot when Tools > Updates is On (default) and it is newer than the image core, built for the same core ABI and its sha256 sums match (`CORE.manifest`, written by `zpod-dev --core`). See [BOOT-UPDATES.md](./BOOT-UPDATES.md) |
| bad launcher in `/data` | reboot (the overlay is not re-applied at boot) | automatic: the boot guard (launcher not up within 60 s), a boot counter (2 tries) and `zpod-launcher-run` (4 exits within 60 s) mark it failed and run the image core; by hand: hold Home while powering on (skips it once), Tools > Updates Off or `zpod-app core off` |
| bundled apps | installed by first boot only | updated at every boot when the image's `.zpa` is newer (`zpod-app update --if-newer`); removed apps stay removed |
| where an app appears in the menu | `category` (Home or Tools) | `[menu] path` in `manifest.toml` (e.g. `Tools/Radio`), see [APP-MANIFEST.md](./APP-MANIFEST.md); `zpod-app tree` |
| launcher restart without a black frame | from the second `--core` on (the new launcher keeps the panel when `/run/zpod/stop-kind` says `restart`; the image's launcher blanks it once) | same |

The image also gets the launcher and `zpod-app` with the control socket
built in once `apps/tools/build-arm.sh` runs for the next image, so a fresh
card does not need `--core` at all.

`zpod-core-overlay status` shows which binaries are active; `zpod-core-overlay
off --restart` goes back to the image's.

## Pi OS image

`zpod-app-push.sh -u pi app.zpa` works the same way (tar over ssh, then
`sudo zpod-app install --force`). `--restart` and `zpod-app logs` need a
launcher and `zpod-app` built from this commit; `--core` refuses on Pi OS
(its root file system is writable: install `apps/dist/dev/core/*` into
`/usr/lib/zpod/bin` with sudo and restart the launcher service). Not tested
on a Pi OS card yet.

## Not done here (later roadmap steps)

* `install --dev` with a "DEV" badge, signing policy (bundle v1, A3).
* Hot reload inside a process: not planned; restart in place is the model.
* macOS simulator build (`zpod-hal` does not compile on macOS, ROADMAP 5.5).
* A new app needs `apps/packages/<id>/manifest.toml` (+ icon) and a crate
  named `zpod-<id>` (or set `ZPOD_APP_PKG`); `zpod-app new` templates are A6.
