# ZPOD music player (M4)

`/opt/zpod/apps/music/bin/music`: a Rust UI on the 320x240 LCD that drives
**MPD** (Music Player Daemon). Browse by artist, album or playlist, see album
art, and control everything with the ZPOD buttons.

## Why MPD + a Rust client (backend decision)

| Option | Verdict |
|---|---|
| **MPD + Rust UI client** (chosen) | MPD decodes in optimised C (libFLAC, libmpg123, ffmpeg) with low CPU on the ARM1176. It has a tag database, gapless playback, playlists, replay of the queue after reboot, and album art (`albumart` for folder covers, `readpicture` for embedded art). Playback **survives the UI exiting** (go to the clock while music plays), and `mpc` lets you control it remotely over SSH. The UI talks plain text over `/run/mpd/socket`, so no audio libraries are cross-linked into our binaries. Debian packages it (`mpd`, `mpc`). |
| Pure-Rust player (symphonia + cpal/alsa) | No daemon, but we would have to write the library DB, queue, gapless and resume ourselves. Software FLAC/MP3 decoding in Rust on ARMv6 without NEON is heavier, and it means linking ALSA into a static binary. Keep this for the ESP32-S3 port; `zpod_core::AudioSink` is the hook for it. |
| mopidy / gmediarender | Python/GStreamer: too heavy for 512 MB and a single 1 GHz ARMv6 core. |

## Layout

* `crates/music` lib `music_core` (`no_std` + `alloc`): model, screens,
  navigation and rendering on any `embedded-graphics` `DrawTarget<Rgb565>`.
  The `MusicBackend` trait is the only way it reaches the player, so the same
  core can later sit on an ESP32 decoder.
* bin `music` (feature `std`): `backend.rs` implements `MusicBackend` over
  `zpod-mpd`. `art.rs` fetches covers on a background thread, decodes
  JPEG/PNG, centre-crops to 120x120 RGB565 and caches them in
  `~/.cache/zpod/music-art/`.
* Status bar: time, battery % (MAX17048), Wi-Fi bars, USB badge, volume and
  play state, shared with every app through `zpod-hal::sys::StatusSource`
  (sources polled every 5 to 20 s).

## Screens and buttons

Menu: Now Playing, Artists, Albums, Playlists, Queue, Shuffle all, Update library.

| Button | Lists | Now Playing |
|---|---|---|
| Up / Down | move (autorepeat) | **Up = repeat** (cycles the modes), **Down = shuffle** on/off |
| Right / OK / Circle | open, or play from this track | Right = next track (hold = seek +10 s); **OK = open the queue** |
| Left / X | back | Left = previous track; X = back |
| Square | add to queue | |
| Triangle | jump to Now Playing | |
| Play | play / pause (starts the queue if stopped) | play / pause |
| Home | exit to the launcher (music keeps playing) | same |
| Vol+ / Vol- | zpod-buttons sets ALSA 'Digital'; the app only shows a toast | same |

These follow the vendor conventions: Up = repeat one, Down = shuffle, Home =
menu, OK = queue, X = cancel.

Albums are grouped by AlbumArtist, falling back to Artist. Tracks sort by
disc and track number. Names sort case-insensitively, ignoring a leading "The ".

## Music location

`/srv/zpod/media/music/<Artist>/<Album>/<NN Title>.flac|mp3|ogg|m4a...`, with
`cover.jpg` or `folder.jpg` for folder art, or embedded art. Playlists (`.m3u`)
go in `/srv/zpod/media/playlists`. MPD has `auto_update` on, and the transfer
tools also ask it to rescan. See FILE-TRANSFER.md.

## Simulator

```sh
cd apps
python3 tools/gen-test-library.py /tmp/zpod-mpd   # 17 tracks, 5 albums, 4 artists, 2 playlists
mpd /tmp/zpod-mpd/mpd.conf && mpc -h /tmp/zpod-mpd/socket -w update
. tools/sim/env.sh; ZPOD_SIM_SCRIPT=tools/sim/music.txt ./target/debug/music
```

Screenshots: `screenshots/music-*.png`.

## Known limits / follow-ups

* Not yet run on the Pi: MPD CPU load and the art decode time on ARMv6 are
  estimates. Covers are decoded once and then cached.
* No search (no keyboard). Long lists scroll with autorepeat only, so an
  A-Z jump would help big libraries.
* The Albums view asks MPD for every album at once. This is untested beyond the
  17-track fixture; expect a delay on the first open with thousands of albums.
