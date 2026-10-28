# Tagging, cover art and pushing albums (`zpod-tag-and-push.sh`)

The ZPOD music app browses by **tags** (Album artist / Artist, Album, Track),
not by folder. Untagged files are missing from Artists and Albums, and albums
without art show a blank cover. `scripts/zpod-tag-and-push.sh` fills in missing
tags and covers on **copies** of your files, then pushes them to the ZPOD.

`scripts/zpod-push-music.sh` is still the plain copier for music that is
already tagged and has art. Use `zpod-tag-and-push.sh` for anything from the NAS
or other loose rips.

## Quick start (NAS folder, from the Mac)

```sh
cd ~/src/zpod      # or wherever the repo is
NAS="/Volumes/Media/Music/to file/Miles Davis - FLAC"

scripts/zpod-tag-and-push.sh -n "$NAS"            # 1. dry run: what it would do, writes nothing
scripts/zpod-tag-and-push.sh --review "$NAS"      # 2. proposes covers, writes review.html, no push
open ~/zpod-stage/library/review.html             # 3. look at every proposed cover
scripts/zpod-tag-and-push.sh --accept-art "$NAS"  # 4. tag, embed covers, push, verify, rescan
```

You can also pass one album folder, or several. If the folder names do not
name the artist, add `--artist "Name"`.

The NAS must be mounted first: in Finder use **Go > Connect to Server** with
`smb://NAS-4x12.local/Media`, which mounts at `/Volumes/Media`. The script
never writes to the source.

## What it does, step by step

1. **Find albums.** Every folder that directly holds audio files
   (`.flac .mp3 .m4a .ogg .opus`) is an album. A `Disc 1` / `CD2` /
   `Disc - 1` subfolder counts as a disc of its parent album.
2. **Plan.** It reads the existing tags and derives what is missing (see the
   conventions below). It prints a per-album plan: artist and where it came
   from, album, year, the file names it parsed, and the art status.
3. **Stage.** It copies the files into `~/zpod-stage/library/<Artist>/<album folder>/`
   (`--work` or `$ZPOD_STAGE` changes the location). Names are normalised to
   NFC so accented titles do not show up twice on Linux. A manifest
   (`.zpod-manifest.json`) records each source's size and mtime, so re-runs copy
   only new or changed files. NAS flags and ACLs are not copied.
4. **Tag.** Only on staged copies, and only files missing one of
   artist/album/title/tracknumber. It fills in only the **missing** fields:
   artist, albumartist, album, title, tracknumber, discnumber, date, and genre
   when known. Good existing tags are never touched unless you pass `--force`.
5. **Cover art.** Skipped if the album already has `cover.*`, `folder.*` or
   `front.*` (jpg/png) or embedded art. Otherwise it searches Apple's public
   iTunes catalogue for artist + album and downloads a 600x600 JPEG as
   `cover.proposed.jpg`. It records where the cover came from in
   `.cover-source.txt`. **Nothing is pushed while a proposed cover is
   waiting** (exit code 3). With `--accept-art` the proposal becomes
   `cover.jpg` and is embedded in every track.
6. **Push.** Sets permissions to 644 for files and 755 for dirs, so MPD can read
   them; NAS files are often 700. Then it runs
   `rsync -rtp` to `/srv/zpod/media/music/<Artist>/<album folder>/`, skipping
   dot files and `*.proposed.*`. It runs `chmod` again on the device.
7. **Verify.** SHA-256 of every pushed file, on the Mac and on the ZPOD
   (`--verify size` is faster; `--verify none` skips it). A mismatch exits 4.
8. **Rescan.** `mpc -w update`, then `mpc stats`, and the album count per artist.

A log of each run goes to `~/zpod-stage/library/.logs/`. Passwords are never
asked for or stored. SSH runs in batch mode, so set up a key once with
`ssh-copy-id <you>@10.12.194.1`.

## Folder and file name conventions it understands

| Where | Patterns | Result |
|---|---|---|
| Album folder | `(1958) Miles Davis - Milestones`, `(1949-1951) Artist - Album`, `1959 - Album`, `Album (1959)`, `[1959] Album` | year 1958 (first year of a range), artist, album |
| | trailing `[320 kbps]`, `[FLAC]`, `(24-96)` ... | dropped |
| | `Somethin' Else (Cannonball Adderley album)` | album `Somethin' Else` |
| Artist | `--artist` > existing tags > `Artist - Album` in the folder name > parent folder (`Artist/Album`) | parent names like `Music`, `Compilations`, `to file`, `_in itunes` are ignored, and suffixes like `Miles Davis - FLAC` become `Miles Davis` |
| Track file | `05 Dr. Jackle`, `01 - So What`, `01. So What`, `1-05 Title` (disc 1, track 5) | track number, title (a leading `Artist - ` is removed) |
| Disc folder | `Disc 1`, `CD2`, `Disc - 1` | discnumber |
| Genre | `--genre`, or a path component named like a genre (`Jazz/`, `Classical/` ...) | only if the file has none |

If no artist can be found, the album is reported as `TODO: pass --artist` and
skipped; the other albums still go.

## Options

```
--artist NAME   --genre G
-n/--dry-run    plan only, writes nothing anywhere (cover search shows its pick)
--review        plan + download proposed covers + review.html, no tagging, no push
--accept-art    accept the proposed covers (after review)
--cover 'ALBUM=SPEC'   per album; ALBUM is the folder name or album name;
                SPEC = itunes:<collectionId> | https://...jpg | /path/to/image.jpg | none
--no-art        never fetch or embed art        --art-only  only art, keep tags
--force         overwrite existing tags from names, re-pick covers
--no-push       stage + tag only                --verify sha256|size|none
--work DIR      -H host  -u user  --dest DIR (default /srv/zpod/media/music)
```

## The review workflow (cover accuracy)

**Automatic cover matching is best-effort.** In the first hand run, 5 of 19
Miles Davis covers were wrong (for example "Relaxin'" matched "Workin'", and
"Miles, The New Miles Davis Quintet" matched "Workin'"). Matching now ignores
edition notes such as "(Remastered)", needs the artist to match, and grades
every proposal:

* **high**: the title matches exactly (ignoring edition notes) and the artist matches.
* **medium**: the title is close (85% or more) and the artist matches.
* **low**: anything else. **Assume a low match is wrong.** For example, "Miles"
  (1955) still matches "Steamin'".
* **manual**: you chose it with `--cover`.

Even a high match can be a different edition with different artwork, such as a
mono or 2026 remaster. That is why the default is review, then confirm:

1. Run `--review`, then `open ~/zpod-stage/library/review.html`. It shows each album's
   proposed cover, grade and the Apple Music page it came from.
2. To fix a wrong cover, pick one of these:
   * find the right album on music.apple.com; the number at the end of its URL
     is the id. Then `--cover "(1955) Miles Davis - Miles, The New Miles Davis Quintet=itunes:1443120095"`
   * save any image over `.../<album>/cover.proposed.jpg` in the work dir (it is kept on re-runs)
   * `--cover "<album>=/path/to/scan.jpg"` or `--cover "<album>=none"` for no cover
3. Re-run with `--accept-art`. Each album's `.cover-source.txt` keeps a trail:
   proposed, accepted, the iTunes id and URL, and the grade.

`--no-art` pushes without covers; covers can be added later with `--art-only`.

## Fixing tags or art by hand later

The staged copy is the master for what is on the ZPOD. Edit it, then re-run the
script, or plain-push it with `zpod-push-music.sh`:

```sh
W=~/zpod-stage/library
PY=~/zpod-stage/.venv/bin/python          # has mutagen
# change a tag on every track of an album
$PY - "$W/Miles Davis/(1958) Miles Davis - Milestones" <<'PY'
import sys, glob, mutagen
for f in glob.glob(sys.argv[1] + '/*.flac'):
    m = mutagen.File(f, easy=True); m['album'] = ['Milestones']; m.save()
PY
# replace a cover: put the new image in place, then re-run with --force --cover ALBUM=<file> --accept-art
```

After editing, run `scripts/zpod-tag-and-push.sh <same source>`. Changed files
are re-sent, then MPD rescans. On the device, `mpc update "Miles Davis"` rescans
one artist. The music app caches 120x120 thumbnails in
`~/.cache/zpod/music-art/`; delete that folder if an old cover sticks.

## Known limits

* Cover art comes only from the iTunes Search API, which is US storefront,
  public and needs no key. Rare albums may be missing or match a different edition.
* Tags come from names only; there is no acoustic fingerprinting or MusicBrainz
  lookup. Track titles are as good as the file names.
* Box-set rips keep their original numbers (e.g. Milestones tracks 05-10).
  They sort correctly but do not start at 1.
* Covers that already exist as source sidecars are copied, not embedded. Only
  covers this tool fetches are embedded.
* `--force` re-derives every field from names, so it can make good tags worse.
  Use it per album.
* Albums stay staged in `~/zpod-stage` (same size as the source) until you
  delete them. Deleting the stage only loses the edits.
* Needs Python 3. Mutagen is installed once into `~/zpod-stage/.venv` if the
  system Python lacks it.
