#!/bin/bash
# ZPOD first-boot login setup (M4.1+). Images never contain a password or a key.
#
# Boot partition (FAT, "bootfs" on the Mac) files read at boot:
#   userconf.txt   stock Raspberry Pi OS "name:<openssl passwd -6 hash>".
#                  Applied by userconfig.service (userconf-pi), not by this script.
#   zpod-ssh.pub   SSH *public* keys, authorized_keys format (one per line).
#                  Appended to ~/.ssh/authorized_keys of the first user (uid 1000),
#                  then the file is deleted.
#   zpod-user.txt  optional: login name for a key-only setup (first line,
#                  e.g. "paul"). Default: keep the first user's name ("pi").
#
# Runs before userconfig.service. If the first user was never set up (no
# userconf.txt and no Imager customisation) and zpod-ssh.pub is present, it
# finishes the setup for key-only login: rename (optional), login shell, end of
# the rename wizard. The password stays locked, so sudo is made passwordless for
# that user (same as stock Raspberry Pi OS 010_pi-nopasswd). With a valid
# userconf.txt the stock service sets the name and password as usual and the
# keys below follow the user (usermod -m moves the home directory).
set -euo pipefail
BOOT="${ZPOD_BOOT_DIR:-/boot/firmware}"
PUB="$BOOT/zpod-ssh.pub"
NAMEF="$BOOT/zpod-user.txt"
RENAME_CONF="${ZPOD_RENAME_CONF:-/etc/ssh/sshd_config.d/rename_user.conf}"
USERCONF="${ZPOD_USERCONF:-/usr/lib/userconf-pi/userconf}"
SUDOERS="${ZPOD_SUDOERS:-/etc/sudoers.d/010_zpod-nopasswd}"
log() { echo "zpod-firstuser: $*"; }

[ -f "$PUB" ] || { log "no zpod-ssh.pub; nothing to do"; exit 0; }

# Keep only well-formed public key lines (never private keys).
keys="$(tr -d '\r' < "$PUB" | sed 's/^\xef\xbb\xbf//' | grep -E '^(ssh-(ed25519|rsa|dss)|ecdsa-sha2-nistp(256|384|521)|sk-ssh-ed25519@openssh\.com|sk-ecdsa-sha2-nistp256@openssh\.com) [A-Za-z0-9+/=]+( .*)?$' || true)"
if grep -q 'PRIVATE KEY' "$PUB"; then log "zpod-ssh.pub contains a PRIVATE key: ignored"; keys=""; fi
n=$(printf '%s' "$keys" | grep -c . || true)
if [ "$n" -eq 0 ]; then
  log "zpod-ssh.pub has no valid public key lines; left in place for inspection"
  exit 0
fi

user="$(getent passwd 1000 | cut -d: -f1)"
[ -n "$user" ] || { log "no uid 1000 user"; exit 1; }

valid_userconf() {
  local f line
  for f in "$BOOT/userconf" "$BOOT/userconf.txt"; do
    [ -f "$f" ] || continue
    line="$(head -n1 "$f" | tr -d '\r')"
    [[ "$line" =~ ^[a-z][a-z0-9-]{0,31}:.+ ]] && return 0
  done
  return 1
}

if [ -e "$RENAME_CONF" ] && ! valid_userconf; then
  new="$user"
  if [ -f "$NAMEF" ]; then
    cand="$(head -n1 "$NAMEF" | tr -d '\r[:space:]')"
    if [[ "$cand" =~ ^[a-z][a-z0-9-]{0,31}$ ]] && [ "$cand" != root ]; then new="$cand"
    else log "zpod-user.txt: invalid name; keeping '$user'"; fi
  fi
  if [ "$new" != "$user" ] && getent passwd "$new" >/dev/null; then
    log "user '$new' already exists; keeping '$user'"; new="$user"
  fi
  log "first user not set up: enabling key-only login for '$new' (was '$user')"
  "$USERCONF" "$new" ""          # rename + /bin/bash + cancel-rename (ends the wizard)
  user="$new"
  ( umask 0337
    printf '# ZPOD key-only setup (zpod-firstuser): no password set, so sudo is passwordless.\n%s ALL=(ALL) NOPASSWD: ALL\n' "$user" > "$SUDOERS.tmp" )
  if command -v visudo >/dev/null && ! visudo -cqf "$SUDOERS.tmp"; then
    log "sudoers check failed; not installed"; rm -f "$SUDOERS.tmp"
  else
    chmod 0440 "$SUDOERS.tmp"; mv -f "$SUDOERS.tmp" "$SUDOERS"
  fi
  rm -f "$NAMEF"
fi

home="$(getent passwd "$user" | cut -d: -f6)"
grp="$(id -gn "$user")"
install -d -m 0700 -o "$user" -g "$grp" "$home/.ssh"
ak="$home/.ssh/authorized_keys"
touch "$ak"
added=0
while IFS= read -r k; do
  [ -n "$k" ] || continue
  grep -qxF "$k" "$ak" || { printf '%s\n' "$k" >> "$ak"; added=$((added + 1)); }
done <<< "$keys"
chown "$user:$grp" "$ak"; chmod 0600 "$ak"
rm -f "$PUB"; sync
log "installed $added new key(s) for '$user' ($n in zpod-ssh.pub); zpod-ssh.pub removed"
