#!/bin/sh
# Copy music from a Mac (or Linux) to the ZPOD and rescan the library.
# For untagged files or albums without cover art, use zpod-tag-and-push.sh
# instead (docs/MUSIC-TAGGING.md).
#
#   scripts/zpod-push-music.sh [options] <folder-or-file>...
#     -H host     ZPOD address (default: 10.12.194.1 over USB, else zpod.local)
#     -u user     SSH user (default: $ZPOD_USER, else your login name; the image's
#                 first user is renamed at first boot, see docs/FIRST-LOGIN.md)
#     --http      upload with curl to the zpod-drop server instead of rsync/SSH
#     --token T   zpod-drop token (Wi-Fi only; USB needs none)
#     --delete    rsync --delete inside each pushed album folder (mirror)
#     -n          dry run (rsync only)
#
# Each argument lands as <music>/<basename>: push "~/Music/Nova Echo" to get
# /srv/zpod/media/music/Nova Echo/<albums>. SSH asks for the password
# unless you have a key (ssh-copy-id <user>@10.12.194.1). Passwords are never
# stored by this script.
set -eu
HOST=""; USER_=${ZPOD_USER:-$(id -un)}; MODE=rsync; TOKEN=""; DELETE=""; DRY=""
DEST=/srv/zpod/media/music
while [ $# -gt 0 ]; do
  case "$1" in
    -H) HOST=$2; shift 2 ;;
    -u) USER_=$2; shift 2 ;;
    --http) MODE=http; shift ;;
    --token) TOKEN=$2; shift 2 ;;
    --delete) DELETE=--delete; shift ;;
    -n) DRY=-n; shift ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    --) shift; break ;;
    -*) echo "unknown option $1" >&2; exit 2 ;;
    *) break ;;
  esac
done
[ $# -ge 1 ] || { sed -n '2,20p' "$0"; exit 2; }
if [ -z "$HOST" ]; then
  if ping -c1 -t1 10.12.194.1 >/dev/null 2>&1 || ping -c1 -W1 10.12.194.1 >/dev/null 2>&1; then
    HOST=10.12.194.1
  else
    HOST=zpod.local
  fi
fi
echo "ZPOD: $HOST ($MODE)"

if [ "$MODE" = rsync ]; then
  # macOS stores names decomposed (NFD); Linux/MPD expect composed (NFC).
  # rsync 3.x can convert; Apple's bundled rsync/openrsync cannot.
  ICONV=""
  if [ "$(uname)" = Darwin ]; then
    if rsync --version 2>/dev/null | head -1 | grep -q 'version 3'; then
      ICONV="--iconv=utf-8-mac,utf-8"
    else
      echo "note: install rsync 3 (brew install rsync) for correct accented names, or use --http" >&2
    fi
  fi
  # Permissions: macOS openrsync and rsync 2.6.9 ignore --chmod (M5: files
  # arrived as 700 and MPD could not read them), so --chmod is only a hint for
  # rsync 3; the remote chmod below is what makes every pushed file 644 and
  # every folder 755, whatever rsync the Mac has.
  q() { printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"; }
  fix=""
  for src in "$@"; do
    src=${src%/}
    # shellcheck disable=SC2086
    rsync -rtvh --progress --partial --modify-window=2 $DRY $DELETE $ICONV \
      --exclude '.*' --exclude 'Thumbs.db' --chmod=Du=rwx,Dgo=rx,Fu=rw,Fgo=r \
      "$src" "$USER_@$HOST:$DEST/"
    name=$(basename "$src")
    [ "$(uname)" = Darwin ] && name=$(printf '%s' "$name" | iconv -f UTF-8-MAC -t UTF-8 2>/dev/null || printf '%s' "$name")
    fix="$fix $(q "$DEST/$name")"
  done
  if [ -z "$DRY" ]; then
    # folders 755, files 644 (not chmod X: files that arrived 700 would get +x), then rescan
    # shellcheck disable=SC2029
    ssh "$USER_@$HOST" "find$fix -type d -exec chmod 755 {} + -o -type f -exec chmod 644 {} + && echo 'permissions fixed (dirs 755, files 644)' && mpc -q update && echo 'library update started'"
  fi
else
  base="http://$HOST:8080/music"
  hdr=""; [ -n "$TOKEN" ] && hdr="X-Zpod-Token: $TOKEN"
  nfc() { if [ "$(uname)" = Darwin ]; then printf '%s' "$1" | iconv -f UTF-8-MAC -t UTF-8; else printf '%s' "$1"; fi; }
  urlenc() { # percent-encode everything except unreserved and '/'
    printf '%s' "$1" | od -An -tx1 -v | tr -s ' \n' ' ' | tr ' ' '\n' | while read -r h; do
      [ -n "$h" ] || continue
      case $h in
        2d|2e|2f|5f|7e|3[0-9]|4[1-9a-f]|5[0-9a]|6[1-9a-f]|7[0-9a]) printf "\\$(printf %03o "0x$h")" ;;
        *) printf '%%%s' "$h" ;;
      esac
    done
  }
  for src in "$@"; do
    src=${src%/}; parent=$(dirname "$src")
    find "$src" -type f ! -name '.*' | while IFS= read -r f; do
      rel=$(nfc "${f#"$parent"/}")
      printf '%s ... ' "$rel"
      curl -fsS -T "$f" ${hdr:+-H "$hdr"} "$base/$(urlenc "$rel")" || echo "FAILED"
    done
  done
  echo "zpod-drop rescans the library automatically a few seconds after the last upload."
fi
