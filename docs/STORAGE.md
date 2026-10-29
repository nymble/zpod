# SD card layout (M5): small fixed root + `/data`

From `v0.6.0-m5` the image has three partitions:

| # | Mount | Type | Size | Holds |
|---|---|---|---|---|
| 1 | `/boot/firmware` (volume **bootfs** on the Mac) | FAT32 | 512 MiB | firmware, kernel, `config.txt`, `cmdline.txt`, first-boot files (`userconf.txt`, `zpod-ssh.pub`, `zpod-user.txt`, `zpod-wifi.txt`, `zpod-usb.txt`) |
| 2 | `/` | ext4 `rootfs` | **fixed**: rootfs + 30 % + 512 MiB, at least 3 GiB | the OS, apps (`/opt/zpod/apps`), MPD database |
| 3 | `/data` | ext4 `zpoddata` | 256 MiB in the image, **grown to the end of the card at first boot** (max 2 TiB) | `/data/media/music`, `/data/media/playlists`, `/data/zpod/appdata` (app settings, saves, alarm) |

`/data/media` is bind-mounted on **`/srv/zpod/media`**, so every path in the
docs and tools (`/srv/zpod/media/music`) is unchanged. App data uses
`ZPOD_DATA_DIR=/data/zpod/appdata` (set in `zpod-launcher.service`; apps
inherit it).

Why: on a 128 GB card the M4 layout put 120 GB of music on the root file
system. Now the OS can be re-flashed or replaced (for example by the
Buildroot image) without touching the music partition's layout, a full music
card cannot fill the root file system, and fsck of the OS stays quick.

## First boot

1. `cmdline.txt` has **no `resize`** any more, so the initramfs does not grow
   the root partition (it would collide with partition 3).
2. `zpod-data-grow.service` (before `local-fs-pre.target`) runs
   `parted resizepart 3 <last sector>` when there is more than 8 MiB free
   after partition 3. It runs on every boot but only acts once (or again if
   the card is cloned onto a bigger one).
3. fstab mounts `/data` with `x-systemd.growfs`, so `systemd-growfs` grows the
   ext4 file system online to the new partition size. The file system was
   made with `-E resize=` for 2 TiB, 4 KiB blocks, no reserved blocks and
   one inode per 64 KiB.
4. `zpod-data-setup.service` makes sure the directories exist with the right
   owners (music: uid 1000, playlists: 1000:mpd 0775, appdata: `zpod`).
5. `/data/media` is bind-mounted on `/srv/zpod/media`; MPD, the drop server
   and the launcher start after it.

Both mounts are `nofail`: if `/data` cannot be mounted the ZPOD still boots
(music folder empty, app settings fall back to
`/var/lib/zpod/home/.local/share/zpod`).

## Card sizes

* **Minimum 8 GB** (the image is 4.0 GB / 3.76 GiB uncompressed, 425 MiB as .xz).
* Paul's 128 GB card: `/data` becomes ~115 GiB.
* Cards **under 2 TB** use all their space. Larger cards: `/data` stops at
  2 TiB (MBR partition table limit; the rest is unused).
* `ZPOD_LAYOUT=single` at export time builds the old 2-partition layout.

## Checking it

```sh
lsblk -o NAME,SIZE,FSTYPE,LABEL,MOUNTPOINT /dev/mmcblk0
findmnt /data /srv/zpod/media
journalctl -b -u zpod-data-grow -u systemd-growfs@data -u zpod-data-setup
```

The launcher's Settings > Diagnostics > Storage & build shows **Root /**,
**Data**, **Music ("on data")** and **Boot**.

## Moving music from an M4 card

The M4 layout kept music on the root partition. Copy it off first
(`rsync -a paul@10.12.194.1:/srv/zpod/media/ ~/zpod-media-backup/`), write the
M5 image, then push it back with `scripts/zpod-push-music.sh`.
