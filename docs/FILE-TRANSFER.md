# Getting files onto the ZPOD (M4)

Music lives in **`/srv/zpod/media/music`** (owned by `pi`), and playlists in
`/srv/zpod/media/playlists`. MPD watches the folder (`auto_update yes`), and
every tool below also asks it to rescan.

There are two links, both already working since M2/M3:

| Link | Pi address | Typical speed | Notes |
|---|---|---|---|
| USB cable (g_ether / RNDIS-ECM) | `10.12.194.1` (/28, DHCP to the Mac) | ~3-5 MB/s (USB 2 on the Zero, CPU-bound) | trusted, no extra auth for the drop server |
| Wi-Fi (wlan0) | DHCP; `zpod.local` via avahi | ~1-2 MB/s (2.4 GHz, single antenna) | drop server needs a token |

(Speeds are estimates for the ARM1176 with SSH encryption. They have not been
measured on the device yet. See TEST-M4.md step 6.)

## 1. rsync / scp over SSH (primary)

From the Mac, in the repo:

```sh
scripts/zpod-push-music.sh ~/Music/"Nova Echo"           # USB if connected, else zpod.local
scripts/zpod-push-music.sh -H zpod.local ~/Music/Album   # force Wi-Fi
scripts/zpod-push-music.sh -n ~/Music/Album              # dry run
```

It runs `rsync -rtvh --partial --progress` into `/srv/zpod/media/music/` and
then `mpc update`. You can resume interrupted copies by running it again. SSH
prompts for the `pi` password (********) unless you install a key once:
`ssh-copy-id pi@10.12.194.1`.

**Untagged rips or albums without art (e.g. from the NAS):** use
`scripts/zpod-tag-and-push.sh` instead. It stages copies, fills in missing
tags and cover art (with a review step), pushes, verifies SHA-256 and
rescans. See [MUSIC-TAGGING.md](MUSIC-TAGGING.md).

Manual equivalents:

```sh
rsync -rtvh --progress ~/Music/"Artist" pi@10.12.194.1:/srv/zpod/media/music/
scp -r ~/Music/"Artist/Album" pi@10.12.194.1:/srv/zpod/media/music/Artist/
ssh pi@10.12.194.1 mpc update
```

**Accented file names (NFD vs NFC):** macOS file systems hand out names in
decomposed Unicode (NFD), while Linux keeps whatever bytes arrive. MPD plays
both, but the same album copied twice in different forms shows up twice. The
script passes `--iconv=utf-8-mac,utf-8` when rsync 3.x is installed
(`brew install rsync`). Apple's bundled rsync/openrsync cannot convert, so the
script warns; `--http` mode converts names with `iconv` before uploading.

## 2. Drop server (light, no SSH): `zpod-drop`

A ~400 kB static Rust HTTP server, socket-activated by systemd on port 8080.
It starts on the first connection and exits after 10 idle minutes, so it costs
nothing while unused.

* Browser: open **http://10.12.194.1:8080/** over USB and pick an album
  folder or files. The page keeps the `Artist/Album/` structure and
  normalises names to NFC.
* curl: `curl -T "01 Song.flac" "http://10.12.194.1:8080/music/Artist/Album/01%20Song.flac"`
* Script: `scripts/zpod-push-music.sh --http ~/Music/Album`
* `POST /update` rescans; `GET /status` returns free space as JSON. Uploads also
  trigger a rescan 3 s after the last file.

Safety:

* Only audio, cover and playlist extensions are accepted. Traversal (`..`),
  hidden files and absolute paths are rejected.
* Each file is written as `.name.part`, fsynced, then renamed, so a pulled
  cable never leaves half a FLAC in the library.
* There is a 2 GiB per-file cap, and uploads are refused when less than 64 MB
  would remain free.
* Runs as `pi` with `ProtectSystem=strict` and can write only
  `/srv/zpod/media`.
* **Over USB** (peer in 10.12.194.0/28) no auth is needed: only the cable owner
  can reach it.
* **Over Wi-Fi** uploads are refused unless `/etc/zpod/drop.conf` has a
  `token = ...` (8+ chars, generated on the Pi; the file explains how). Then use
  `http://zpod.local:8080/?token=<token>` or `--token`. The token is plain HTTP
  on your LAN; it is a door latch, not TLS.

The launcher's **System, File transfer** screen shows the current addresses
and whether the drop server is listening.

## 3. Apps

The same links carry app packages. See APPS.md:
`scripts/zpod-app-push.sh tetris-1.0.0.zpa`.

## USB CDC-ACM (serial over USB): evaluated, not adopted

Paul asked to evaluate CDC-ACM because the UART is unusable. Findings:

* The Zero's single USB controller (dwc2) runs in peripheral mode, and today
  loads **`g_ether`** (verified working in M2/M3: 10.12.194.1/28). A serial
  console would need either `g_cdc` (ECM + ACM composite) or a configfs gadget
  with `ecm`/`rndis` + `acm` functions.
* Swapping `g_ether` for `g_cdc` drops RNDIS. Macs use ECM, so a Mac would
  still work, but it changes the verified link and the M3 `zpod-usb-kick`
  workaround would need re-validating. A configfs composite gadget is the
  clean way (script in `/usr/local/sbin`, `libcomposite`), but it is a
  boot-path change that can only be validated on hardware.
* What CDC-ACM would buy: a login console (`getty@ttyGS0`) when networking is
  broken. It brings **no file-transfer benefit**: zmodem/kermit over ACM is
  slower and clumsier than rsync over the existing USB network.
* **Recommendation:** keep `g_ether` for M4. As a follow-up, try a configfs
  gadget `ecm + acm` on a spare SD card, with `getty@ttyGS0.service`, and check
  that the Mac sees both `/dev/cu.usbmodem*` and the network interface before
  changing the image. It is not implemented here and not tested.
