# A/B system updates (keep `/data`)

From **v0.11.0-br-ab-test6** a new system is installed onto the card while the
ZPOD runs, without reflashing and without touching `/data` (music, apps,
accounts, Wi-Fi, SSH keys). Flashing a full `.img` is now only for new cards
or recovery: **it wipes the card**.

Status: implemented and tested in emulation (`buildroot/tests/test_ab_br.sh`,
QEMU). **Not yet tried on hardware**; the first real update is the test in
[TEST-BR-TEST6.md](./TEST-BR-TEST6.md).

## Layout

The card layout has not changed since the first Buildroot images, so a test5
card needs no repartitioning:

| | | |
|---|---|---|
| p1 | FAT `ZPODBOOT`, 64 MB | firmware, `config.txt`, boot files |
| p2 | system A, squashfs, 256 MB | |
| p3 | system B, squashfs, 256 MB | until test6 an unused copy of A |
| p4 | `/data`, ext4, rest of card | never written by an update |

The Pi Zero W firmware has no `tryboot` (newer Pi models only), so the choice of
system is made with the firmware's `os_prefix=`, which prefixes the kernel,
the device tree, `overlays/` and `cmdline.txt` (also `cmdline=`):

```
/boot/firmware/
  config.txt            ends with "include zpod-slot.txt"
  zpod-slot.txt         os_prefix=slot-a/ or slot-b/ (written by zpod-ab)
  zImage, *.dtb, overlays/, cmdline.txt   system A, used when there is no
                                          zpod-slot.txt (as on test5)
  slot-a/  zImage *.dtb overlays/ cmdline.txt (root=/dev/mmcblk0p2 zpod.slot=a)
           cmdline-safe.txt VERSION
  slot-b/  same, root=/dev/mmcblk0p3 zpod.slot=b
```

`zpod-slot.txt`:

```
os_prefix=slot-b/        <- the system to boot
[gpio12=0]               <- Square held at power-on:
os_prefix=slot-a/        <-   the other one
[all]
```

The firmware ignores an `os_prefix` whose folder lacks a kernel and `.dtb`
and boots the top-level files (system A) instead.

## Buttons at power-on

- **Square** (GPIO12): boot the other system for this boot. If the new
  system is still on trial, the trial is abandoned and the running (old)
  system is kept.
- **Home** (GPIO24): unchanged; safe mode (USB gadget, no `/data` core
  update) for whichever system boots.

## Trial and fallback

`zpod-ab install` writes the new system to the slot that is **not** running
and marks it as on trial (`/data/zpod/ab/state`). On each of the first two
boots of the new system, `zpod-ab boot` (from `zpod-data-early`, early in
sysinit) counts the try and starts a guard: if the launcher is running with
its control socket for 20 s within 120 s, the new system is committed;
otherwise `zpod-slot.txt` is switched back and the ZPOD reboots into the old
system. A third boot still on trial reverts before anything else.

Not caught: a kernel that never reaches user space (hangs or panics before
sysinit). Then hold **Square** while powering on, or put the card in a
computer and delete `zpod-slot.txt` on the `ZPODBOOT` drive (boots system A
from the top-level files).

## Bundle `.zab`

A tar file built with the image (`zpod-<version>.zab`, ~45 MB):

```
AB.manifest        format=1 board=pi0w version git date abi, file=<sha256> <path>
zpod-ab            the installer (so a test5 card, which has none, can run it)
rootfs.squashfs
boot/              zImage, bcm2708-rpi-zero-w.dtb, overlays/, cmdline.txt, config.txt
```

`zpod-ab verify` checks the format, board, the listed files and every
sha256, that the root is a squashfs and that `config.txt` has both includes.
The bundle is not signed (only hashed); install it only from a release you
trust.

## Install

From the Mac, over SSH (Wi-Fi or USB), no reboot:

```
scripts/zpod-ab-push --verify-only ~/Downloads/zpod-v0.11.0-br-ab-test6.zab
scripts/zpod-ab-push ~/Downloads/zpod-v0.11.0-br-ab-test6.zab
```

It copies the bundle to `/data/zpod/ab/`, checks its sha256 there, unpacks
it and runs the bundled `zpod-ab install`:

1. writes `rootfs.squashfs` to the other partition (refused if it is
   mounted, too small, or the running system is itself on trial) and reads
   it back to compare;
2. builds `slot-<x>.new/` on the FAT partition and renames it to `slot-<x>/`
   (kernel copied last); an update into A also refreshes the top-level files;
3. **test5 migration:** if the card has no `slot-a/` yet, copies the running
   system's top-level boot files there, so Square can return to it;
4. replaces `config.txt` with the bundle's (previous kept as
   `config.txt.prev`) and writes `zpod-slot.txt`.

Then reboot (Power > Reboot). `zpod-ab status` shows the running system,
both versions, and the trial state; `/run/zpod/ab.log` has the log.

On the device (root): `zpod-ab status`, `verify <bundle>`, `install
<bundle>`, `commit` (keep the running system now), `switch` (next boot: the
other system, no reboot).

## Interaction with core updates (`docs/BOOT-UPDATES.md`)

A core update in `/data/zpod/core` is keyed to the image by ABI and version,
so after an A/B switch it is re-evaluated: an update older than the new
system is skipped. Home-at-power-on still skips only the `/data` core
update, never the system choice.

## Limits

- Not tried on hardware yet: `os_prefix` with this firmware, the
  `[gpio12=0]` override, and `cmdline=` in a gpio filter with a prefix.
- No signing; hash checks only.
- The FAT partition holds the firmware plus up to three kernel sets (top
  level, `slot-a`, `slot-b`), about 20 MB of 64 MB.
- 256 MB per system; the current root is about 90 MB.
