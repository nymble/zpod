# Getting files onto the ZPOD (M4, M5, M6)

Music lives in **`/srv/zpod/media/music`** (owned by the first user, uid
1000), and playlists in `/srv/zpod/media/playlists`. On M5 images that folder
is a bind mount of **`/data/media`** on the large data partition (see
[STORAGE.md](STORAGE.md)); the path you copy to is unchanged. MPD watches the
folder (`auto_update yes`), and every tool below also asks it to rescan.

## The File transfer screen (M5)

Top menu > **File transfer** shows, live (refresh every 3 s):

| Row | Shows |
|---|---|
| USB | `10.12.194.1` and `ssh/scp/rsync <user>@10.12.194.1` (or "not connected") |
| Wi-Fi | the Wi-Fi IP and `zpod.local` (mDNS via avahi) |
| Browser | the drop server URL when it is listening (`http://10.12.194.1:8080/`; on Wi-Fi it needs the token) |
| rsync | the optional rsync share: off / ON `rsync://10.12.194.1/music/` / not installed |
| Space | free / total of the music folder, `(/data)` with the M5 layout |
| Library | song count and the last MPD scan time, or **rescanning...** while MPD updates |
| Newest files | the 4 most recently modified files MPD knows (`find … sort -Last-Modified`) |

Buttons: **OK = rescan the library now** (`mpc update`), **Square = rsync
share on/off**, Left/X = back. Copies by any method are picked up
automatically too (MPD `auto_update`, the drop server and `zpod-push-music.sh`
also trigger a rescan); OK is for when something did not show up.

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

It runs `rsync -rtvh --partial --progress` into `/srv/zpod/media/music/`,
then makes every pushed folder 755 and file 644 on the ZPOD (`find … chmod`
over SSH; M6 fix: Apple's openrsync ignores `--chmod`, so M5 left files 700
and MPD could not read them), then `mpc update`. You can resume interrupted copies by running it again. SSH
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

A ~600 kB static Rust HTTP server, socket-activated by systemd on port 8080.
**M6: off by default**, switched on from Tools > Drop box / Book catalog (section 2a).
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

The launcher's **File transfer** screen shows the current addresses and
whether the drop server is listening.

## 2a. Drop box and book catalog (M6, both off by default)

M6 makes `zpod-drop` a small file server with two parts, each switched in
**Tools** (OK = on/off). The socket on port 8080 only listens while at least
one is on; M5's always-on music upload is now part of the drop box.

| Part | Switch | URL | What |
|---|---|---|---|
| Drop box | Tools > Drop box | `http://<ZPOD>:8080/` | web page: browse and download `music`, `books` and `inbox`; upload any file into **`/data/inbox`** (max `max_upload_mb`, default 1024 MiB per file, and never closer than 64 MiB to a full `/data`); M5 music upload (`PUT /music/…`, `zpod-push-music.sh --http`) |
| Book catalog | Tools > Book catalog | `http://<ZPOD>:8080/opds` | **OPDS 1.2** catalog of **`/data/books`** (EPUB, PDF, MOBI/AZW3, CBZ/CBR, FB2, DjVu, TXT): All books, Newest, By author. EPUB title/author/language/cover come from the book's OPF; other files use `Author - Title.ext` names |

`<ZPOD>` is `10.12.194.1` over USB, `10.42.0.1` on the ZPOD's own hotspot
(Tools > Hotspot), or `zpod.local` on your home Wi-Fi.

Who may connect:

* **USB** (10.12.194.0/28) and the **ZPOD hotspot** (10.42.0.0/24, WPA2
  with a key generated on the ZPOD): no token.
* **Home Wi-Fi**: refused unless `/etc/zpod/drop.conf` has `token = …`
  (8+ characters, made on the Pi; see the file). Browser:
  `http://zpod.local:8080/?token=<token>`; e-reader apps: user anything,
  **password = the token** (HTTP Basic), or `?token=` in the catalog URL.

E-reader apps: KOReader (Search > OPDS catalog > +), Thorium Reader,
Marvin, Moon+ Reader, Calibre's "Get books" do not need an account; add
`http://10.42.0.1:8080/opds` while connected to the ZPOD hotspot.

Put books on the ZPOD with `scp book.epub paul@10.12.194.1:/data/books/`, by
moving them from `/data/inbox` after a drop box upload, or with rsync. The
catalog re-reads the folder on each request (EPUB metadata cached by
size + mtime).

Files: `/etc/zpod/drop.conf` (`dropbox`, `opds`, `books_dir`, `inbox_dir`,
`max_upload_mb`, `token`), units `zpod-drop.socket` / `zpod-drop.service`
(uid 1000, `ProtectSystem=strict`, writes only `/srv/zpod/media` and
`/data/inbox`).

## 2b. Optional rsync share (M5, off by default)

For copying from a computer without SSH keys or passwords: a plain rsync
daemon with one module, `music`.

* **Off by default.** Turn it on/off on the File transfer screen (Square), or
  `sudo systemctl enable --now zpod-rsyncd.socket` / `disable --now`.
* Listens **only on the USB link** (`10.12.194.1:873`, `hosts allow =
  10.12.194.0/28`), never on Wi-Fi. Socket-activated: no process runs until a
  connection arrives.
* **No password**: anyone who plugs a cable into the ZPOD while it is on can
  read and write the music folder. That is the same trust level as the drop
  server over USB. Leave it off when you do not need it; SSH (section 1) is
  the secure way.
* Writes as uid 1000 inside a chroot of the music folder; `mpc update` runs
  after each transfer.

```sh
rsync -rtv --progress ~/Music/"Artist" rsync://10.12.194.1/music/
rsync rsync://10.12.194.1/music/                 # list
```

Samba was not added: it costs ~40 MB and a resident daemon for no gain over
rsync/drop on a single-user device.

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
