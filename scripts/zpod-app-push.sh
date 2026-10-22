#!/bin/sh
# Install or update a ZPOD app package (.zpa) from a Mac, over USB or Wi-Fi.
#   scripts/zpod-app-push.sh [-H host] [--force] app-1.2.0.zpa [more.zpa...]
# Uses scp + ssh; sudo on the Pi asks for the pi password (not stored).
set -eu
HOST=10.12.194.1; FORCE=""
while [ $# -gt 0 ]; do
  case "$1" in
    -H) HOST=$2; shift 2 ;;
    --force) FORCE=--force; shift ;;
    *) break ;;
  esac
done
[ $# -ge 1 ] || { sed -n '2,4p' "$0"; exit 2; }
scp "$@" "pi@$HOST:/tmp/"
cmds=""
for p in "$@"; do
  b=$(basename "$p")
  # install if new, update if already installed
  cmds="$cmds id=\$(zpod-app info /tmp/$b | sed -n 's/^id: *//p'); if zpod-app info \"\$id\" >/dev/null 2>&1; then sudo zpod-app update $FORCE /tmp/$b; else sudo zpod-app install $FORCE /tmp/$b; fi; rm -f /tmp/$b;"
done
ssh -t "pi@$HOST" "$cmds zpod-app list"
