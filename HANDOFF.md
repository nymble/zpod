# ZPOD handoff (2026-11-01)

A short note so another assistant or developer can pick up the build. Read
`README.md`, `PLAN.md`, `docs/BUILDROOT.md` and `docs/TEST-BR-M6.md` next.

## Where things are

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

## Building

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

## Talking to the device

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

## Rules (from Paul)

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

## State at handoff

Test3 is on Paul's 128 GB card and working: correct version, red LED off at idle,
Wi-Fi joined home network, clock synced, Bluetooth scan, music copied (382 tracks).
Not yet checked on hardware: hotspot device count + status-bar mode icon, Wi-Fi
Channel use chart, switching back to Client mode.

## Next jobs, in order

(Done 2026-11-05: app dev loop, ROADMAP-APPS step 1, `docs/DEV-LOOP.md`.)

1. **Non-destructive updates**: A/B system updates (RAUC style) into p2/p3 that
   never touch `/data`, with boot fallback if the new slot fails. Test3 should be
   the last image that needs a full reflash.
2. **App manager** under Tools: add/remove apps packaged under `/data/apps`.
3. Hotspot web console for the iPad (music upload, Wi-Fi setup, status).
4. USB landing page that interrogates whatever is plugged in (Bash Bunny-style
   tools later); active Wi-Fi tools; typing a Wi-Fi password on the device; mDNS.
5. Merge PR #33 into `main`.
