#!/bin/bash
# Test overlay/usr/lib/zpod/zpod-firstuser.sh in a throwaway root container:
#   docker run --rm -v "$PWD":/src:ro debian:trixie bash /src/tests/test_firstuser.sh
# Uses a stand-in for userconf-pi's rename (same usermod/groupmod steps).
set -euo pipefail
S=/src/overlay/usr/lib/zpod/zpod-firstuser.sh
T=$(mktemp -d); B=$T/boot; mkdir -p $B $T/sshd
fail() { echo "FAIL: $*"; exit 1; }
cat > $T/userconf <<'U'
#!/bin/sh
new=$1; old=$(getent passwd 1000 | cut -d: -f1)
if [ "$old" != "$new" ]; then usermod -l "$new" "$old"; usermod -m -d "/home/$new" "$new"; groupmod -n "$new" "$old"; fi
usermod -s /bin/bash "$new"; [ -z "${2:-}" ] || echo "$new:$2" | chpasswd -e; rm -f "$ZPOD_RENAME_CONF"
U
chmod +x $T/userconf
export ZPOD_BOOT_DIR=$B ZPOD_RENAME_CONF=$T/sshd/rename_user.conf ZPOD_USERCONF=$T/userconf ZPOD_SUDOERS=$T/sudoers
# M6 boot notice: off until "booted once" exists
export ZPOD_BOOTED_ONCE=$T/booted-once ZPOD_NOTICE=$T/run/boot-notice
reset() { userdel -r paul 2>/dev/null || true; userdel -r pi 2>/dev/null || true; groupdel pi 2>/dev/null || true
  useradd -u 1000 -m -s /usr/sbin/nologin pi; passwd -l pi >/dev/null; touch $ZPOD_RENAME_CONF; rm -f $B/* $T/sudoers; }
K1="ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExampleKeyOnlyForTestsAAAAAAAAAAAAAAAAAA paul@M1"
K2="ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTY= x"

# 1. no file: no-op
reset; bash $S >/dev/null; test -e $ZPOD_RENAME_CONF || fail "1: rename conf removed"
# 2. key-only + zpod-user.txt=paul: renamed, bash, sudoers, keys, files gone
reset; printf '%s\r\n# comment\n%s\nnot a key\n' "$K1" "$K2" > $B/zpod-ssh.pub; echo paul > $B/zpod-user.txt
bash $S
getent passwd paul | grep -q ':1000:.*:/home/paul:/bin/bash$' || fail "2: user"
[ "$(wc -l < /home/paul/.ssh/authorized_keys)" = 2 ] || fail "2: key count"
[ "$(stat -c '%a %U' /home/paul/.ssh/authorized_keys)" = "600 paul" ] || fail "2: ak perms"
[ "$(stat -c '%a' /home/paul/.ssh)" = 700 ] || fail "2: .ssh perms"
grep -qx 'paul ALL=(ALL) NOPASSWD: ALL' $T/sudoers && [ "$(stat -c %a $T/sudoers)" = 440 ] || fail "2: sudoers"
test ! -e $B/zpod-ssh.pub -a ! -e $B/zpod-user.txt -a ! -e $ZPOD_RENAME_CONF || fail "2: leftovers"
# 3. re-run with the same key: no duplicate
printf '%s\n' "$K1" > $B/zpod-ssh.pub; bash $S >/dev/null
[ "$(wc -l < /home/paul/.ssh/authorized_keys)" = 2 ] || fail "3: duplicate"
# 4. valid userconf.txt present: no rename/sudoers, keys installed for the current user (stock service renames later)
reset; echo "paul:\$6\$salt\$hash" > $B/userconf.txt; echo "$K1" > $B/zpod-ssh.pub
bash $S >/dev/null
getent passwd pi >/dev/null && test -e $ZPOD_RENAME_CONF && test ! -e $T/sudoers || fail "4: should defer to userconf"
grep -qxF "$K1" /home/pi/.ssh/authorized_keys || fail "4: key"
test -e $B/userconf.txt || fail "4: userconf.txt must stay for the stock service"
# 5. private key: ignored, file kept
reset; printf -- '-----BEGIN OPENSSH PRIVATE KEY-----\n%s\n' "$K1" > $B/zpod-ssh.pub; bash $S >/dev/null
test -e $B/zpod-ssh.pub && test ! -e /home/pi/.ssh/authorized_keys && getent passwd pi >/dev/null || fail "5: private key"
# 6. key-only without zpod-user.txt keeps 'pi'
reset; echo "$K1" > $B/zpod-ssh.pub; bash $S >/dev/null
getent passwd pi | grep -q '/bin/bash$' && grep -qx 'pi ALL=(ALL) NOPASSWD: ALL' $T/sudoers || fail "6: keep pi"
# 7. key-only first boot, then a valid userconf.txt on a later boot (the M5 bug):
#    renamed to userconf's name, password hash set, sudoers removed, keys follow
reset; echo "$K1" > $B/zpod-ssh.pub; bash $S >/dev/null
test -e $T/sudoers -a ! -e $ZPOD_RENAME_CONF || fail "7: key-only setup first"
echo 'paul:$6$salt$latehash' > $B/userconf.txt; bash $S
getent passwd paul | grep -q ':1000:.*:/home/paul:/bin/bash$' || fail "7: renamed to paul"
! getent passwd pi >/dev/null || fail "7: pi still exists"
[ "$(getent shadow paul | cut -d: -f2)" = '$6$salt$latehash' ] || fail "7: password hash"
test ! -e $T/sudoers || fail "7: passwordless sudoers not removed"
test ! -e $B/userconf.txt || fail "7: userconf.txt not deleted"
grep -qxF "$K1" /home/paul/.ssh/authorized_keys || fail "7: key did not follow the home"
# 7b. same, with zpod-ssh.pub added again together with userconf.txt
reset; echo "$K1" > $B/zpod-ssh.pub; bash $S >/dev/null
echo 'paul:$6$salt$latehash' > $B/userconf.txt; echo "$K2" > $B/zpod-ssh.pub; bash $S >/dev/null
[ "$(wc -l < /home/paul/.ssh/authorized_keys)" = 2 ] && test ! -e $T/sudoers -a ! -e $B/zpod-ssh.pub || fail "7b: late userconf + keys"
# 8. user set up normally (no zpod sudoers): a later userconf.txt is left to the stock rules (ignored)
reset; rm -f $ZPOD_RENAME_CONF; usermod -s /bin/bash pi; echo 'paul:$6$salt$x' > $B/userconf.txt; bash $S >/dev/null
getent passwd pi >/dev/null && ! getent passwd paul >/dev/null && test -e $B/userconf.txt || fail "8: must not touch a set-up user"
# 9. key-only, then an invalid userconf.txt: renamed failed_userconf.txt, sudoers kept, still pi
reset; echo "$K1" > $B/zpod-ssh.pub; bash $S >/dev/null
echo 'Paul' > $B/userconf.txt; bash $S >/dev/null
test -e $B/failed_userconf.txt -a ! -e $B/userconf.txt -a -e $T/sudoers && getent passwd pi >/dev/null || fail "9: invalid late userconf"
test ! -e $ZPOD_NOTICE || fail "notice written before the first boot finished"
# 10. M6 notices: key-only first boot (no notice), later boots report late files
reset; rm -f $ZPOD_NOTICE $ZPOD_BOOTED_ONCE; echo "$K1" > $B/zpod-ssh.pub; bash $S >/dev/null
test ! -e $ZPOD_NOTICE || fail "10: first boot must not show a notice"
touch $ZPOD_BOOTED_ONCE
echo 'paul:$6$salt$latehash' > $B/userconf.txt; echo "$K2" > $B/zpod-ssh.pub; bash $S >/dev/null
grep -qx 'userconf.txt: pi is now paul, password set' $ZPOD_NOTICE || fail "10: rename notice"
grep -qx 'sudo now asks for that password' $ZPOD_NOTICE || fail "10: sudo notice"
grep -qx 'zpod-ssh.pub: 1 new SSH key(s) for paul' $ZPOD_NOTICE || fail "10: key notice"
! grep -q 'latehash' $ZPOD_NOTICE || fail "10: hash leaked into the notice"
# 11. normally set-up user + stray userconf.txt: reported as ignored, left in place
reset; rm -f $ZPOD_RENAME_CONF $ZPOD_NOTICE; usermod -s /bin/bash pi; echo 'paul:$6$salt$x' > $B/userconf.txt; bash $S >/dev/null
grep -qx 'userconf.txt ignored: login already set up' $ZPOD_NOTICE && test -e $B/userconf.txt || fail "11: ignored notice"
# 12. late invalid userconf.txt after key-only: notice names the failed_ file
reset; rm -f $ZPOD_NOTICE; echo "$K1" > $B/zpod-ssh.pub; bash $S >/dev/null; rm -f $ZPOD_NOTICE
echo 'Paul' > $B/userconf.txt; bash $S >/dev/null
grep -qx 'userconf.txt: invalid, renamed failed_userconf.txt' $ZPOD_NOTICE || fail "12: invalid notice"
echo "test_firstuser: all 13 cases passed"
