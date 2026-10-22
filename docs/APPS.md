# ZPOD apps, launcher and boot splash (M4)

## Pieces

| Binary | Where | Role |
|---|---|---|
| `zpod-bootsplash` | `/usr/lib/zpod/bin` | earliest LCD frame, progress and boot status lines |
| `zpod-launcher` | `/usr/lib/zpod/bin` | home menu, built-in System screens, runs apps; replaces the Python `zpod-ui` |
| `zpod-app` | `/usr/lib/zpod/bin` (+ `/usr/local/bin` link) | install / update / remove / list packages |
| `zpod-drop` | `/usr/lib/zpod/bin` | HTTP upload server (FILE-TRANSFER.md) |
| apps | `/opt/zpod/apps/<id>/` | `music`, `clock`, `tetris` ship in the image as packages |

**One LCD owner at a time.** The splash owns the panel until the launcher
starts. The launcher then owns it except while an app runs: it draws a launch
card, closes spidev and the GPIO lines, `exec`s the app in its own process
group and waits. When the app exits, it re-opens the panel. The panel is
initialised with SWRESET only once per boot (`/run/zpod/lcd-initialised`);
every later owner does a *warm* init that skips the reset, so hand-offs do not
flash.

## App layout and manifest

```
/opt/zpod/apps/tetris/
  manifest.toml
  icon.png          32x32 PNG with alpha (optional; a default is used)
  bin/tetris        static ARMv6 binary (or a #! script)
  ...               any assets the app wants (read-only)
```

```toml
[app]
id = "tetris"            # must equal the directory name; a-z 0-9 '-', <= 32
name = "Tetris"          # menu label, <= 40 chars
version = "1.0.0"        # semver; update refuses downgrades unless --force
exec = "bin/tetris"      # relative to the app dir (no "..", no absolute)
args = []                # optional
icon = "icon.png"        # optional
description = "Falling blocks: SRS, hold, ghost, high scores"
min_core = "0.5.0"       # oldest ZPOD core (/etc/zpod/core-version) it supports
order = 30               # optional menu position (lower first, then by name)
arch = "arm-unknown-linux-musleabihf"   # informational
```

Unknown keys are rejected (`deny_unknown_fields`), so a typo cannot silently
grant anything.

### Runtime contract

* The app runs as user **`zpod`** (groups spi, gpio, i2c, input, audio,
  video), never as root. Its cwd is the app dir; `HOME=/var/lib/zpod/home`.
  The environment carries `ZPOD_APP_ID`, `ZPOD_APP_DIR` and
  `ZPOD_CORE_VERSION`.
* It owns the LCD (open `zpod_hal::Platform::open()`: warm init) and reads
  buttons from evdev "zpod-buttons" (no grab). It must **not** apply
  Vol+/Vol-: zpod-buttons already did; redrawing the value is fine.
* **Home** means "back to the launcher": exit with status 0. A non-zero exit
  or a crash shows a toast in the launcher.
* **Holding Home for 3 s** makes the launcher SIGTERM the app's process group,
  then SIGKILL it 1.5 s later: a hung app can always be escaped.
* Persistent data: `$ZPOD_DATA_DIR` or `~/.local/share/zpod/<app>/`
  (`zpod_hal::FileStorage`). Caches: `~/.cache/zpod/`.

## Packages (`.zpa`)

A `.zpa` is a gzip'd tar of the app directory with `manifest.toml` at the top.

```sh
zpod-app pack apps/dist/stage/tetris dist/     # -> dist/tetris-1.0.0.zpa (reproducible: sorted, root-owned, fixed mtime)
zpod-app check tetris-1.0.0.zpa                # validate without installing
sudo zpod-app install tetris-1.0.0.zpa         # new app
sudo zpod-app update  tetris-1.1.0.zpa         # newer version only (--force to reinstall/downgrade)
zpod-app list
zpod-app info tetris          # or: zpod-app info file.zpa
sudo zpod-app remove tetris
```

From the Mac: `scripts/zpod-app-push.sh [-H zpod.local] app.zpa`, which
installs or updates as appropriate. No image rebuild is needed.

What install guarantees:

1. Extracts into `/opt/zpod/apps/.staging-<id>-<pid>`. Absolute paths, `..`,
   symlinks, hard links and device nodes are rejected, as are more than
   10,000 entries or more than 256 MiB unpacked. File modes are normalised to
   0644/0755.
2. Validates the manifest, checks that `exec` exists, and checks `min_core`
   against `/etc/zpod/core-version`.
3. **Checks every ELF in the package**: it must be 32-bit little-endian ARM,
   and `.ARM.attributes` Tag_CPU_arch must be ARMv6-compatible (v6, v6KZ,
   v6K). An `armv7`/`aarch64`/x86 build is refused with a hint, instead of
   dying with SIGILL on the ARM1176.
4. Swaps the new version in by rename (old version kept until then) and
   fsyncs the directory. A failed install leaves the previous version
   untouched. A lock file serialises concurrent runs.
5. Pokes `/run/zpod/apps-changed`. The launcher re-scans when it is back on
   its home screen.

Not done yet (follow-up): package **signatures** (e.g. minisign/ed25519 key
in `/etc/zpod/trusted-keys`). Today anyone with sudo can install anything,
the same as with `scp` + `sudo`.

## Launcher

Home lists the apps by `order`, then **System**:

| System item | What it does (Python zpod-ui equivalent) |
|---|---|
| Status | time, wlan0/usb0 IP, battery % + volts, volume, DAC present, MPD state, uptime, core version |
| Volume | big readout and bar; Up/Right +3 %, Down/Left -3 % (amixer 'Digital') |
| Wi-Fi info | SSID, signal, IP, power save, usb0 (nmcli/iw); OK refreshes |
| File transfer | USB/Wi-Fi addresses for rsync and the drop page; drop server state |
| Installed apps | versions, ids, BROKEN entries with the reason |
| Play test tone | `timeout 10 speaker-test -D default -t sine -f 440 -c 2` (pauses MPD first); select again to stop |
| Reboot / Shutdown | confirm dialog (OK = yes, X = cancel) |

Keys: Up/Down move; OK/Right/Circle open; X/Left/Square back; **Home** goes to
the home screen. **Triangle** opens the clock (as in M3). **Play/Pause** toggles
MPD when it has a queue, otherwise the M3 test tone. **Vol+/-** shows a
"Vol NN%" toast (the volume itself was set by zpod-buttons).

### Shutdown and end screen (M3 behaviour kept)

On SIGTERM the launcher stops the running app and then decides what to show:

* The stop kind comes from `/run/zpod/stop-kind` (written by its own power
  menu, or by a BusyBox shutdown script), otherwise from `systemctl
  list-jobs` (poweroff/halt vs reboot/kexec, the same test as zpod_ui.py).
* `/etc/zpod/ui.conf` `[shutdown] screen = message | off`:
  * poweroff + `message`: "ZPOD off / Safe to power off / Slide the power
    switch off. / Halted <date>", backlight stays on.
  * reboot: "Rebooting...".
  * otherwise (plain stop, or `screen = off`): black, DISPOFF, SLPIN,
    backlight GPIO27 low.
* The M3 `system-shutdown/zpod-lcd-off` backstop is unchanged.
* `zpod-launcher --end-screen poweroff|reboot|off` draws just that frame, for
  init systems without systemd.

Power actions use `systemctl reboot|poweroff` when systemd is running,
otherwise BusyBox `reboot`/`poweroff`.

## Boot splash

**Goal:** pixels on the LCD as early as userspace allows, then a live view of
boot until the launcher appears.

* `zpod-bootsplash.service`: `DefaultDependencies=no`, `WantedBy=sysinit.target`,
  so it starts in the first wave after PID 1, in parallel with udev. It does
  not wait for udev; it polls for `/dev/spidev0.0` every 25 ms (spidev appears
  when udev coldplug loads `spi-bcm2835`/`spidev` from `dtparam=spi=on`).
* Static musl binary (opt-level z, ~400 kB) with no Python and no shared
  libraries: exec to first frame should take tens of ms after spidev exists.
  On a cold boot this is a full init plus one 150 kB frame (~40 ms of SPI at
  32 MHz).
* Draws the ZPOD logo, a progress bar and the last 7 status lines:
  * With systemd: `journalctl -b -f -o cat _PID=1` (PID 1's "Starting X... /
    Started X." lines, with unit names replaced by their descriptions).
  * Without systemd (BusyBox/Buildroot): `/run/zpod/splash.fifo`. Init
    scripts run `zpod-bootsplash msg "Mounting data"` or
    `zpod-bootsplash progress 60` (non-blocking if no splash runs).
* Progress is the max of (lines seen / lines in the previous boot) and
  (elapsed / previous boot duration), self-calibrating via
  `/var/lib/zpod/bootsplash.cal`. It is capped at 97 % until the hand-off.
* Hand-off: the launcher unit `Conflicts=zpod-bootsplash.service` (systemd),
  or the launcher SIGTERMs the pid from `/run/zpod/bootsplash.pid`
  (no systemd). The splash exits leaving its last frame, and the launcher
  warm-inits over it with no flash.
* **Timing is logged:** `/run/zpod/splash-first-frame` holds CLOCK_BOOTTIME
  (seconds since kernel start) at the first frame. Also in the journal:
  `journalctl -u zpod-bootsplash`. Compare with `systemd-analyze` and the
  M3 zpod-ui first frame. Expected (unmeasured): roughly 6-10 s after power-on
  (≈2-3 s firmware + 3-5 s kernel to PID 1 + under 1 s to spidev), versus the
  M3 Python UI, which appears only after multi-user.

### Going even earlier (follow-ups, not implemented)

1. **Kernel-side panel:** the `panel-mipi-dbi-spi` DRM driver with a firmware
   command file can replay our exact init (including MADCTL 0xE8, no reset,
   DC=GPIO25). That gives a framebuffer and boot logo about 2-3 s after
   power-on, before userspace. The catch: spidev0.0 would then belong to the
   DRM driver, so every app would need to render through `/dev/fb0` or DRM
   instead of spidev. That is an architectural change to evaluate on
   hardware.
2. **initramfs with the splash binary**: starts before the root fs is
   mounted. Small win on RPi OS; natural on Buildroot.
3. Trim kernel boot (`quiet`, fewer modules), already partly done in M3.

## Portability (ESP32-S3 / Buildroot)

* App cores are `no_std` + `alloc` libraries: `tetris-core`, `clock_core`,
  `music_core`, `zpod-ui`, `zpod-core`, `zpod-drivers`, `zpod-panel`. They
  draw on `embedded-graphics` `DrawTarget<Color = Rgb565>` and reach the
  device only through the `zpod-core` traits (`InputSource`, `Clock`,
  `Storage`, `Battery`, `AudioSink`) and the per-app traits (`MusicBackend`).
  `apps/tools/check-nostd.sh` builds them for `thumbv7em-none-eabihf` as a CI
  check.
* `zpod-drivers`: MAX17048 and DS3231 on embedded-hal 1.0 `I2c`.
* `zpod-panel`: a mipidsi 0.10 `Model` for the ZPOD ILI9340 that replays the
  M2/M3 init byte for byte. On ESP32 it plugs into `mipidsi` with esp-hal SPI.
* Linux-only: `zpod-hal` (spidev/gpio-cdev through linux-embedded-hal, evdev,
  sysfs, MPD), the launcher, `zpod-app` and `zpod-drop`. The launcher and the
  splash do not need systemd (pid file, FIFO, stop-kind file,
  `--end-screen`); BusyBox init scripts are in `apps/system/busybox/`.
