#!/bin/sh
# Tag, add cover art to, and push albums to the ZPOD (docs/MUSIC-TAGGING.md).
#
#   scripts/zpod-tag-and-push.sh [options] <album-folder | folder-of-albums>...
#     --artist NAME       album artist when the folder names do not say
#     --genre G           genre for files that have none
#     --review            show the plan, download proposed covers, stop (no push)
#     -n, --dry-run       show the plan only; write nothing anywhere
#     --accept-art        use the proposed covers (after you reviewed them)
#     --cover 'ALBUM=SPEC'  cover for one album: itunes:<id> | URL | image file | none
#     --no-art            never fetch or embed cover art
#     --art-only          only add cover art, leave tags alone
#     --force             overwrite existing tags and re-pick covers
#     --no-push           stage and tag only
#     --verify sha256|size|none   check pushed files (default sha256)
#     --work DIR          staging dir (default $ZPOD_STAGE or ~/zpod-stage/library)
#     -H host  -u user    ZPOD (default 10.12.194.1 over USB, else zpod.local; $ZPOD_USER or you)
#
# Sources are never modified: files are copied to the work dir, tagged there,
# then rsynced to /srv/zpod/media/music/<Artist>/<Album folder>/ and MPD rescans.
# Typical use:  --review first, check review.html, then run again with --accept-art.
# Passwords are never asked for or stored; use an SSH key (ssh-copy-id).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
case "${1:-}" in -h|--help|"") sed -n '2,24p' "$0"; [ -n "${1:-}" ] && exit 0 || exit 2 ;; esac

# A Python 3 with mutagen: system one if it has it, else a private venv (one-time pip install).
py=python3
if ! "$py" -c 'import mutagen' 2>/dev/null; then
  venv=${ZPOD_TAG_VENV:-$HOME/zpod-stage/.venv}
  if [ ! -x "$venv/bin/python" ] || ! "$venv/bin/python" -c 'import mutagen' 2>/dev/null; then
    echo "note: installing mutagen into $venv (one time)" >&2
    python3 -m venv "$venv"
    "$venv/bin/pip" install -q --disable-pip-version-check mutagen
  fi
  py=$venv/bin/python
fi
exec "$py" "$here/zpod_tag_push.py" "$@"
