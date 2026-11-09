#!/bin/sh
# Tests for board/zpod/rootfs-overlay/usr/lib/zpod/zpod-firstuser-br (run as
# root in a Linux container: it chowns). Uses temp dirs, never the real /data.
#   sh buildroot/tests/test_firstuser_br.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
S=$HERE/../board/zpod/rootfs-overlay/usr/lib/zpod/zpod-firstuser-br
pass=0 fail=0
ok() { if eval "$2"; then pass=$((pass+1)); echo "ok   $1"; else fail=$((fail+1)); echo "FAIL $1"; fi; }
KEY='ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestKeyOnlyForUnitTests0000000000000000 test@mac'
# a syntactically valid SHA-512 crypt string (not a real password's hash)
HASH='$6$testsalt$0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ./0123456789abcdefghijklmnopqrstuv'
setup() {
	T=$(mktemp -d); mkdir -p $T/boot $T/etc $T/home
	printf 'root:x:0:0:root:/root:/bin/sh\nzpod:x:999:999:ZPOD apps:/home/zpod:/bin/sh\n' > $T/etc/passwd
	printf 'root:*::::::::\nzpod:*:::::::\n' > $T/etc/shadow
	printf 'root:x:0:\naudio:x:29:zpod\nvideo:x:44:zpod\ninput:x:101:zpod\nsudo:x:27:\nzpod:x:999:\n' > $T/etc/group
	export ZPOD_BOOT_DIR=$T/boot ZPOD_ETC=$T/etc ZPOD_HOMES=$T/home \
		ZPOD_BOOTED_ONCE=$T/booted-once ZPOD_NOTICE=$T/notice
}
run() { sh "$S" > $T/log 2>&1; }

setup; echo "$KEY" > $T/boot/zpod-ssh.pub; run
ok "1 key-only: key for root" "grep -qxF '$KEY' $T/etc/ssh/authorized_keys"
ok "1 key-only: pub deleted" "[ ! -e $T/boot/zpod-ssh.pub ]"
ok "1 key-only: no uid 1000" "! awk -F: '\$3==1000' $T/etc/passwd | grep -q ."

setup; echo "paul:$HASH" > $T/boot/userconf.txt; echo "$KEY" > $T/boot/zpod-ssh.pub; run
ok "2 userconf: paul uid 1000" "grep -q '^paul:x:1000:1000:paul:$T/home/paul:/bin/sh' $T/etc/passwd"
ok "2 userconf: hash set" "awk -F: '\$1==\"paul\"{print \$2}' $T/etc/shadow | grep -qxF '$HASH'"
ok "2 userconf: groups" "grep -q '^audio:x:29:zpod,paul' $T/etc/group && grep -q '^sudo:x:27:paul' $T/etc/group"
ok "2 userconf: key for paul" "grep -qxF '$KEY' $T/home/paul/.ssh/authorized_keys"
ok "2 userconf: key for root" "grep -qxF '$KEY' $T/etc/ssh/authorized_keys"
ok "2 userconf: owner" "[ \$(ls -n $T/home/paul/.ssh/authorized_keys | awk '{print \$3}') = 1000 ]"
ok "2 userconf: deleted" "[ ! -e $T/boot/userconf.txt ]"
ok "2 userconf: no hash in log" "! grep -q 'testsalt' $T/log"
cp $T/etc/passwd $T/p1; run
ok "2b rerun is a no-op" "cmp -s $T/p1 $T/etc/passwd"

setup; echo paul > $T/boot/zpod-user.txt; echo "$KEY" > $T/boot/zpod-ssh.pub; run
ok "3 key-only user: paul uid 1000" "grep -q '^paul:x:1000:' $T/etc/passwd"
ok "3 key-only user: no password" "grep -q '^paul:\*:' $T/etc/shadow"
ok "3 key-only user: NOPASSWD sudo" "grep -q '^paul ALL=(ALL) NOPASSWD: ALL' $T/etc/sudoers.d/010_zpod-nopasswd"
ok "3 key-only user: key" "grep -qxF '$KEY' $T/home/paul/.ssh/authorized_keys"
ok "3 key-only user: name file deleted" "[ ! -e $T/boot/zpod-user.txt ]"

echo "stella:$HASH" > $T/boot/userconf.txt; run
ok "4 late userconf: renamed" "grep -q '^stella:x:1000:1000:paul:$T/home/stella:' $T/etc/passwd && ! grep -q '^paul:' $T/etc/passwd"
ok "4 late userconf: hash" "grep -q '^stella:\\\$6\\\$testsalt' $T/etc/shadow"
ok "4 late userconf: home moved" "[ -f $T/home/stella/.ssh/authorized_keys ]"
ok "4 late userconf: groups renamed" "grep -q '^audio:x:29:zpod,stella' $T/etc/group && grep -q '^stella:x:1000:' $T/etc/group"
ok "4 late userconf: NOPASSWD removed" "[ ! -e $T/etc/sudoers.d/010_zpod-nopasswd ]"

setup; echo "paul:secret" > $T/boot/userconf.txt; run
ok "5 invalid userconf -> failed_userconf.txt" "[ -f $T/boot/failed_userconf.txt ] && [ ! -e $T/boot/userconf.txt ]"
ok "5 invalid: no user" "! grep -q '^paul:' $T/etc/passwd"

setup; printf '%s\n%s\n' '-----BEGIN OPENSSH PRIVATE KEY-----' "$KEY" > $T/boot/zpod-ssh.pub; run
ok "6 private key refused" "[ ! -s $T/etc/ssh/authorized_keys ] && [ ! -e $T/boot/zpod-ssh.pub ]"

setup; echo "root:$HASH" > $T/boot/userconf.txt; run
ok "7 root in userconf refused" "grep -q '^root:\*:' $T/etc/shadow && [ -f $T/boot/failed_userconf.txt ]"

# M6 boot notice: nothing on the first boot, one line per file afterwards
setup; echo "$KEY" > $T/boot/zpod-ssh.pub; run
ok "8 first boot: no notice" "[ ! -e $T/notice ]"
touch $T/booted-once; echo "paul:$HASH" > $T/boot/userconf.txt
echo 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAISecondTestKey000000000000000000000000000 b@mac' > $T/boot/zpod-ssh.pub; run
ok "8 later boot: login notice" "grep -qx 'userconf.txt: login paul created' $T/notice"
ok "8 later boot: key notice" "grep -qx 'zpod-ssh.pub: 1 new SSH key(s) for paul' $T/notice"
ok "8 notice has no hash" "! grep -q testsalt $T/notice"
setup; touch $T/booted-once; echo "paul:secret" > $T/boot/userconf.txt; run
ok "8 invalid userconf notice" "grep -qx 'userconf.txt: invalid, renamed failed_userconf.txt' $T/notice"

echo "passed $pass, failed $fail"
[ $fail = 0 ]
