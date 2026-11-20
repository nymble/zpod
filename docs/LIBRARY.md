# Library: music and books between the Mac, Librarian and the ZPOD

Goal: a usable stash of music and books on the ZPOD, with the **master copy
and its metadata kept outside** the ZPOD (the Mac / NAS library that
Librarian manages), and easy movement to and from other players (Apple
Music, e-readers).

## Roles

| Where | Role |
|---|---|
| Mac / NAS (Librarian) | **Master.** Original files, full metadata, cover art, backups, Apple Music. Decides what goes to the ZPOD. |
| ZPOD `/data/media/music` | Stash: copies of albums, played by MPD. Never edited in place by tools; removed only by an explicit command. |
| ZPOD `/data/books` | Stash: copies of books, served by the OPDS catalog (Tools > Wi-Fi tools > Book catalog). |
| ZPOD `/data/var/mpd` | Play state that belongs to the ZPOD: database, queue/position (`state`), stickers (play counts, ratings). |

Rules every tool follows (as `zpod-tag-and-push.sh` already does):

1. Sources are never modified; work happens on staged copies.
2. A dry run (`-n`) exists and writes nothing anywhere.
3. Nothing on the ZPOD is deleted or overwritten by a push. Same name and
   same bytes = skipped; same name, different bytes = **conflict**, listed
   and left alone.
4. Every pushed file is checked by sha256 after the copy.
5. SSH keys only (BatchMode), strict host-key checking as configured.

## test7: what changed

- **MPD state on /data, explicitly.** `mpd.conf` names
  `/data/var/mpd/{tag_cache,state,sticker.sql}`. (It was already on /data
  through the `/var/lib/mpd` link; now it no longer depends on the link.)
  Queue, position, play counts and ratings survive A/B updates.
- **Books push:** `scripts/zpod-books-push.sh` (below).

## Pushing books

```sh
scripts/zpod-books-push.sh -n ~/Books/ZPOD          # plan only
scripts/zpod-books-push.sh ~/Books/ZPOD             # push, verify
scripts/zpod-books-push.sh --into Inbox some.epub    # -> /data/books/Inbox/
```

- A folder keeps its layout below it (`Author/Title.epub`).
- Only book files go: epub pdf mobi azw3 azw kfx cbz cbr fb2 djvu txt (others
  are listed as skipped).
- Exit 0 = all good, 3 = there were conflicts (listed), 4 = a verify failed.
- Offline test: `sh tests/books-push.sh` (a local directory stands in for the
  ZPOD; no SSH).

Reading on the ZPOD itself: not planned for now. The 320x240 screen suits a
book **list** and a "what am I reading" note, not reading. The OPDS catalog
lets a phone or e-reader (KOReader, Thorium, Apple Books via a browser
download) take books from the ZPOD. A minimal reader app (EPUB text only,
saved position) is a later option if Paul wants it.

## Tagging scheme (music)

Unchanged from [MUSIC-TAGGING.md](./MUSIC-TAGGING.md), and chosen so the same
files work in Apple Music:

- Required: `albumartist`, `artist`, `album`, `title`, `tracknumber`
  (`discnumber` for multi-disc), `date` (year). Optional: `genre`,
  `musicbrainz_*` ids when known.
- Layout on the ZPOD: `<Artist>/<album folder>/` as `zpod-tag-and-push.sh`
  stages it; names NFC. The player browses by tags, not folders.
- Cover: embedded art **and** `cover.jpg` (600x600 JPEG) in the album folder.
- Formats: FLAC (lossless master), MP3, AAC/ALAC `.m4a`. Apple Music imports
  MP3 and M4A directly; FLAC goes through an ALAC copy made on the Mac (the
  ZPOD plays FLAC as is).
- Play counts and ratings live in MPD stickers on the ZPOD
  (`/data/var/mpd/sticker.sql`) and are exported, not written into files.

## catalog.v1 (contract with Librarian; plan)

A JSON Lines file the ZPOD can produce (`zpod-catalog`, later) and Librarian
reads, one object per file, so Librarian can see what the stash holds,
whether it matches the master, and what was played:

```json
{"catalog":"zpod","version":1,"device":"zpod","made":"2026-11-20T17:00:00Z","roots":["music","books"]}
{"kind":"music","path":"Miles Davis/Kind of Blue/01 So What.flac","size":31457280,"mtime":1792195200,"sha256":"…","tags":{"albumartist":"Miles Davis","album":"Kind of Blue","title":"So What","track":"1","disc":"1","date":"1959"},"play":{"count":12,"last":"2026-11-18T08:10:00Z","rating":4}}
{"kind":"book","path":"Ursula K. Le Guin/The Dispossessed.epub","size":812345,"mtime":1792195200,"sha256":"…","meta":{"title":"The Dispossessed","author":"Ursula K. Le Guin","lang":"en"}}
```

- First line: header (`catalog`, `version`, `made` in UTC, `roots`).
- `path` is relative to the root (`/data/media/music` or `/data/books`),
  UTF-8 NFC, `/` separators.
- `sha256` may be omitted for a fast listing (`"sha256":null`); Librarian then
  matches by size + mtime and asks for hashes of what it cares about.
- `play` comes from MPD stickers; `rating` 0..5 or absent.
- Unknown keys must be ignored by readers; a breaking change is
  `version: 2`.

## Librarian contract (plan)

| Direction | How | Rules |
|---|---|---|
| Librarian → ZPOD (music) | `zpod-tag-and-push.sh` (tags, covers, push, sha256 verify, MPD rescan) | as above; Librarian passes album folders it picked |
| Librarian → ZPOD (books) | `zpod-books-push.sh` | as above |
| ZPOD → Librarian | `catalog.v1` over SSH (`ssh zpod zpod-catalog > catalog.jsonl`) | read-only on the ZPOD |
| Removal | an explicit list from Librarian, dry run first, then a move to `/data/zpod/trash/<date>/` (not a delete); emptying the trash is a separate step | never automatic |

Apple Music round trip: Librarian (on the Mac) owns the Apple Music side.
ZPOD → Apple Music = take files from the master (not from the ZPOD) and
import. Play counts from `catalog.v1` can be shown in Librarian; writing them
into Apple Music is out of scope for now.
