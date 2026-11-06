# Loading updates from /data at boot

Status: implemented for core 0.10.0 (images after test3). Script:
`buildroot/board/zpod/rootfs-overlay/usr/sbin/zpod-core-overlay`; tests:
`buildroot/tests/test_coreoverlay_br.sh`.

The root file system is read-only squashfs. Two kinds of software can come
from the writable `/data` partition instead of a reflash:

| What | Where | At boot |
|---|---|---|
| **Apps** (`.zpa`) | `/data/zpod/apps/<id>/` (`/opt/zpod/apps`) | always loaded; each one is checked (manifest, `min_core`, launcher API, placement) and runs as its own process, so a bad app is hidden or crashes alone. Bundled apps are updated from the image when the image has a newer version (`zpod-app update --if-newer`, S00zpod-data, log `/data/log/apps-update.log`) |
| **Core update** (launcher, `zpod-app`, button daemon, `zpod-usb-role`) | `/data/zpod/core/` | bind-mounted over the image's files by `zpod-core-overlay boot` in sysinit (zpod-data-early, before the launcher starts), **if loading at boot is on** (default) and the update passes every check below |

## Why on by default

A core update exists on the card only because someone pushed it on purpose
(`scripts/zpod-dev --core`); the dev loop is useless if a reboot silently
drops it. The risk, a launcher that does not start, is handled
automatically (fallback below) and by hand (hold Home at power-on). Turning
it off is one toggle: **Tools > Updates**, or `zpod-app core off`.

## Checks (every boot)

`/data/zpod/core/CORE.manifest`, written by `zpod-dev --core`:

```
format=1
version=0.10.0
build=1794000000          # seconds since 1970 (UTC) at the push
git=491a6b1
abi=1
date=2026-11-05T21:00:00Z
file=<sha256> bin/zpod-launcher
file=<sha256> bin/zpod-app
file=<sha256> zpod_buttons.py
file=<sha256> zpod-usb-role
```

The image records its own core in `/usr/share/zpod/core.id` (version,
build, git, abi, date; written by post-build.sh). Boot loads the update
only if:

1. the kernel command line has no `zpod.nocore` (Home held at power-on);
2. loading at boot is on (`/data/zpod/core/disabled` absent) and the
   update has not failed before (`failed` absent);
3. `CORE.manifest` exists with `format=1`; an update without one (pushed by
   a test3-era `zpod-dev`) is skipped;
4. `abi` equals the image's (`abi` in `core.id`, from `apps/system/etc/zpod/core-abi` in the repo; bumped when the
   launcher <-> image contract changes: paths, init scripts, control socket);
5. it is **newer than the image core**: version, then build time. So
   flashing (or, later, a RAUC slot switch to) a newer image automatically
   retires an older update without deleting it; pushing again makes it
   current again;
6. every listed file is inside the core directory, the binaries are ARM ELF
   and every sha256 matches (about 0.2 s on the Pi Zero W).

Each boot that applies the update counts an attempt
(`/data/zpod/core/boot-attempts`).

## Fallback

* **Guard** (background, started by `boot`): when the launcher has run with
  the same PID and its control socket for 10 s, the attempt count is
  cleared (state `ok`). If not within 60 s, the update is marked failed,
  unmounted and the launcher restarted on the image core (state
  `fallback`).
* **Boot counter**: a third boot with the count still at 2 (e.g. a hang
  that also stopped the guard) marks the update failed without applying it.
* **Crash loop**: `zpod-launcher-run` (the inittab respawn wrapper), after
  the update's launcher starts 4 times within 60 s, runs `zpod-core-overlay fail`, which
  unmounts the update, so the next respawn is the image launcher.
* **Home at power-on**: the firmware's `[gpio24=0]` section in
  `config.txt` (Home is GPIO 24, pressed = 0) selects `cmdline-safe.txt`,
  the normal command line plus `zpod.nocore zpod.safe`. That boot skips the
  update (state `skipped`) without changing anything on the card; the next
  normal boot tries it again. The same section already turns the USB port
  to gadget mode (USB-HOST.md boot guard), so one gesture recovers both.
  *Unverified on hardware: firmware support for `cmdline=` inside a GPIO
  filter.* If it does not work, the guard and counter still recover, and
  `zpod-app core off` over SSH or Wi-Fi does it by hand.

A failed update stays on the card (for logs) until it is pushed again
(`zpod-dev --core` clears `failed`) or turned back on (Tools > Updates /
`zpod-app core on`).

## States (`zpod-app core status`, Tools > Updates)

| state | meaning |
|---|---|
| `ok` | the update is running and passed the guard |
| `pending` | applied this boot; the guard has not decided yet |
| `manual` | applied by hand (`zpod-dev --core`) after boot |
| `fallback` | failed; the image core is running (reason shown) |
| `skipped` | not loaded this boot: Home held, older than the image, wrong ABI, bad checksum, no manifest (reason shown) |
| `off` | loading at boot is off |
| `none` | no update on the card |

```
$ zpod-app core status
boot_load: on
boot_support: 1
active: update
image: 0.10.0 51663e8 2026-11-05
update: 0.10.0 9f0e1aa+dirty 2026-11-06T18:02:11Z
state: ok
reason: update 0.10.0 9f0e1aa+dirty running
attempts:
  /usr/lib/zpod/bin/zpod-launcher <- /data/zpod/core/bin/zpod-launcher
  ...
$ zpod-app core off     # next boot: image core (Tools > Updates: Off)
$ zpod-app core on      # load at boot again; also clears a fallback
```

Other commands (root): `zpod-core-overlay verify` (would it load?),
`apply [--restart] [--force]`, `off [--restart]`, log
`/run/zpod/core-overlay.log`.

test3 has no `core.id` and no `zpod-core-overlay` in the image, so it
cannot load an update at boot: `zpod-dev --core` applies it until the next
reboot, and Tools > Updates says "test3: by hand".

## A/B system updates (test6)

Implemented without RAUC: [AB-UPDATES.md](./AB-UPDATES.md).

The rules are chosen to stay correct once the image is two RAUC slots: the
update is keyed to the image by ABI and version, so booting the other slot
(newer or older) re-evaluates it; a newer slot retires an older update
automatically; the attempt counter and `failed` mark live on `/data` next
to the update, separate from RAUC's slot counters, and the Home-at-power-on
escape skips only the `/data` update, never the slot choice.
