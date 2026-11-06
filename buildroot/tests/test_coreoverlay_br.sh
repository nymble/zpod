#!/bin/sh
# Boot-time core updates (zpod-core-overlay, docs/BOOT-UPDATES.md): the boot
# decision, integrity / ABI / version checks, the attempt counter, the
# launcher health guard and its fallback, the boot option, the Home-at-
# power-on escape hatch. Dry run on a fake /data and /run: never mounts.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
B=$HERE/../board/zpod
CO=$B/rootfs-overlay/usr/sbin/zpod-core-overlay
pass=0 fail=0
ok() { if eval "$2"; then pass=$((pass+1)); echo "ok   $1"; else fail=$((fail+1)); echo "FAIL $1"; fi; }
T=$(mktemp -d)
R=$T/r
elf() { printf '\177ELF\001\001\001\000\000\000\000\000\000\000\000\000\002\000\050\000%s' "$2" > "$1"; chmod 755 "$1"; }
sum() { sha256sum "$1" | cut -d' ' -f1; }
# fresh device: image core 0.10.0 built at 1000, ABI 1; update 0.10.0 built at 2000
mk() {
	rm -rf "$R"; mkdir -p "$R/data/bin" "$R/run" "$R/sys/bin" "$R/sbin" "$R/share"
	for f in bin/zpod-launcher bin/zpod-app zpod_buttons.py; do : > "$R/sys/$f"; done
	: > "$R/sbin/zpod-usb-role"
	printf 'version=0.10.0\nbuild=1000\ngit=img1234\nabi=1\ndate=2026-11-06\n' > "$R/share/core.id"
	elf "$R/data/bin/zpod-launcher" launcher; elf "$R/data/bin/zpod-app" app
	echo 'print("buttons")' > "$R/data/zpod_buttons.py"; echo '#!/bin/sh' > "$R/data/zpod-usb-role"
	manifest "${1:-0.10.0}" "${2:-2000}" "${3:-1}"
	echo "quiet rootwait" > "$R/cmdline"
}
manifest() { # version build abi
	{
		printf 'format=1\nversion=%s\nbuild=%s\ngit=upd5678\nabi=%s\ndate=2026-11-07\n' "$1" "$2" "$3"
		for f in bin/zpod-launcher bin/zpod-app zpod_buttons.py zpod-usb-role; do echo "file=$(sum "$R/data/$f") $f"; done
	} > "$R/data/CORE.manifest"
}
co() {
	ZPOD_CORE_ROOT=$R/data ZPOD_CORE_SYS=$R/sys ZPOD_CORE_SBIN=$R/sbin ZPOD_CORE_RUN=$R/run ZPOD_CORE_ID=$R/share/core.id \
	ZPOD_CORE_CMDLINE=$R/cmdline ZPOD_CORE_DRY=1 ZPOD_CORE_NO_GUARD=1 ZPOD_CORE_GUARD_SECS=2 ZPOD_CORE_SETTLE=1 \
	ZPOD_CORE_SOCK=$R/run/launcher.sock ZPOD_CORE_FAKE_PID=$R/run/pid sh "$CO" "$@"
}
state() { sed -n 's/^state=//p' "$R/run/core-state"; }
reason() { sed -n 's/^reason=//p' "$R/run/core-state"; }
mounted() { grep -qx "$1" "$R/run/mounts.dry" 2>/dev/null; }
nmounts() { n=$(cat "$R/run/mounts.dry" 2>/dev/null | grep -c .); echo "${n:-0}"; }

ok "script is executable, sh -n clean" "[ -x $CO ] && sh -n $CO"

mk; co boot >/dev/null 2>&1
ok "boot: good update -> applied, pending" "[ \"\$(state)\" = pending ] && mounted $R/sys/bin/zpod-launcher && mounted $R/sys/bin/zpod-app && mounted $R/sys/zpod_buttons.py && mounted $R/sbin/zpod-usb-role"
ok "boot: attempt counted" "[ \"\$(cat $R/data/boot-attempts)\" = 1 ]"
ok "status --kv: on, update active, pending" "co status --kv | grep -qx boot_load=on && co status --kv | grep -qx active=update && co status --kv | grep -qx state=pending"
ok "status --kv: versions" "co status --kv | grep -qx 'image=0.10.0 img1234 2026-11-06' && co status --kv | grep -qx 'update=0.10.0 upd5678 2026-11-07'"

# guard: launcher up with its socket for the settle time -> ok, attempts cleared
echo 4242 > "$R/run/pid"; : > "$R/run/launcher.sock"
co guard >/dev/null 2>&1
ok "guard: launcher healthy -> ok, attempts reset" "[ \"\$(state)\" = ok ] && [ ! -e $R/data/boot-attempts ] && mounted $R/sys/bin/zpod-launcher"

# guard: launcher never comes up -> fallback to the image, marked failed
mk; co boot >/dev/null 2>&1; rm -f "$R/run/pid" "$R/run/launcher.sock"
co guard >/dev/null 2>&1
ok "guard: launcher not up -> fallback, unmounted" "[ \"\$(state)\" = fallback ] && [ \"\$(nmounts)\" = 0 ]"
ok "guard: reason says not up within N s" "reason | grep -q 'not up within 2 s'"
ok "guard: update marked failed, option shows off" "[ -s $R/data/failed ] && co status --kv | grep -qx boot_load=off && co status --kv | grep -qx state=fallback"
rm -f "$R/run/mounts.dry"; co boot >/dev/null 2>&1
ok "boot after a fallback: image core, stays failed" "[ \"\$(state)\" = fallback ] && [ \"\$(nmounts)\" = 0 ]"
co enable >/dev/null
ok "enable clears failed + attempts" "[ ! -e $R/data/failed ] && [ ! -e $R/data/boot-attempts ] && co status --kv | grep -qx boot_load=on"
co boot >/dev/null 2>&1
ok "boot after enable: applied again" "[ \"\$(state)\" = pending ] && mounted $R/sys/bin/zpod-launcher"

# guard: launcher restarting all the time (pid changes) -> never settles -> fallback
mk; co boot >/dev/null 2>&1; : > "$R/run/launcher.sock"
( i=0; while [ $i -lt 30 ]; do echo $((1000 + i)) > "$R/run/pid"; sleep 0.2; i=$((i+1)); done ) &
co guard >/dev/null 2>&1; wait
ok "guard: crash-looping launcher never settles -> fallback" "[ \"\$(state)\" = fallback ]"

# guard: crash loop already handled by zpod-launcher-run (fail) -> guard leaves it
mk; co boot >/dev/null 2>&1
co fail "the launcher from the update kept exiting" >/dev/null
echo 77 > "$R/run/pid"; : > "$R/run/launcher.sock"
co guard >/dev/null 2>&1
ok "fail: unmounts, failed reason kept, guard does not report ok" "[ \"\$(state)\" = fallback ] && [ \"\$(nmounts)\" = 0 ] && grep -q 'kept exiting' $R/data/failed"
ok "launcher-run calls fail on a crash loop" "grep -q 'zpod-core-overlay fail' $B/rootfs-overlay/usr/sbin/zpod-launcher-run"

# attempt counter: boots that never reach the guard's ok (power cut, hang)
mk; co boot >/dev/null; rm -f "$R/run/mounts.dry"; co boot >/dev/null; rm -f "$R/run/mounts.dry"
ok "two boots without ok -> attempts 2, still applied" "[ \"\$(cat $R/data/boot-attempts)\" = 2 ] && [ \"\$(state)\" = pending ]"
co boot >/dev/null
ok "third boot: gives up, image core" "[ \"\$(state)\" = fallback ] && [ \"\$(nmounts)\" = 0 ] && grep -q 'did not start on 2 boots' $R/data/failed"

# the boot option
mk; co disable >/dev/null; co boot >/dev/null
ok "disable: boot uses the image" "[ \"\$(state)\" = off ] && [ \"\$(nmounts)\" = 0 ] && co status --kv | grep -qx boot_load=off"
co enable >/dev/null; co boot >/dev/null
ok "enable: boot applies" "[ \"\$(state)\" = pending ]"
mk; co disable >/dev/null; co apply >/dev/null 2>&1
ok "disabled: a manual apply (dev loop) still works now" "mounted $R/sys/bin/zpod-launcher && [ \"\$(state)\" = manual ] && [ -e $R/data/disabled ]"

# escape hatch: Home held at power-on -> cmdline-safe.txt -> zpod.nocore zpod.safe
mk; echo "quiet rootwait zpod.nocore zpod.safe" > "$R/cmdline"; co boot >/dev/null
ok "Home at power-on: skipped once, nothing counted" "[ \"\$(state)\" = skipped ] && reason | grep -q 'Home held' && [ ! -e $R/data/boot-attempts ] && [ \"\$(nmounts)\" = 0 ]"
mk; echo "quiet zpod.nocore" > "$R/cmdline"; co boot >/dev/null
ok "zpod.nocore alone: skipped" "[ \"\$(state)\" = skipped ] && reason | grep -q 'kernel command line'"
ok "config.txt: Home held boots cmdline-safe.txt" "sed -n '/^\[gpio24=0\]/,/^\[all\]/p' $B/config.txt | grep -qx 'cmdline=cmdline-safe.txt'"
ok "post-image writes cmdline-safe.txt with zpod.nocore zpod.safe" "grep -q 'cmdline-safe.txt' $B/post-image.sh && grep -q 'zpod.nocore zpod.safe' $B/post-image.sh"
ok "post-build writes /usr/share/zpod/core.id (version, build, abi)" "grep -q 'usr/share/zpod/core.id' $B/post-build.sh && grep -q '^abi=\$ABI' $B/post-build.sh"
ok "zpod-data-early runs 'zpod-core-overlay boot'" "grep -q 'zpod-core-overlay boot' $B/rootfs-overlay/usr/sbin/zpod-data-early"

# integrity, ABI, version
mk; echo tampered >> "$R/data/bin/zpod-app"; co boot >/dev/null
ok "checksum mismatch -> skipped, names the file" "[ \"\$(state)\" = skipped ] && reason | grep -q 'checksum mismatch: bin/zpod-app' && [ \"\$(nmounts)\" = 0 ] && [ ! -e $R/data/boot-attempts ]"
mk; co verify >/dev/null; echo x >> "$R/data/zpod_buttons.py"; co boot >/dev/null
ok "a change after an earlier good check is caught" "[ \"\$(state)\" = skipped ] && reason | grep -q zpod_buttons.py"
mk 0.10.0 2000 2; co boot >/dev/null
ok "other core ABI -> skipped" "[ \"\$(state)\" = skipped ] && reason | grep -q 'ABI 2, this image has 1'"
mk 0.9.0 9999; co boot >/dev/null
ok "older version than the image -> skipped (newer image wins)" "[ \"\$(state)\" = skipped ] && reason | grep -q 'image core 0.10.0 is newer than the update 0.9.0'"
mk 0.10.0 500; co boot >/dev/null
ok "same version, older build -> skipped" "[ \"\$(state)\" = skipped ] && reason | grep -q 'same version'"
mk 0.11.0 1; co boot >/dev/null
ok "newer version, any build -> applied" "[ \"\$(state)\" = pending ]"
mk 0.10.0 500; co apply >/dev/null 2>&1
ok "manual apply ignores 'newer than the image'" "[ \"\$(state)\" = manual ]"
mk; rm "$R/data/CORE.manifest"; co boot >/dev/null
ok "no CORE.manifest (old dev push) -> skipped at boot" "[ \"\$(state)\" = skipped ] && reason | grep -q 'CORE.manifest'"
co apply >/dev/null 2>&1
ok "no CORE.manifest: manual apply still works (test3 loop)" "mounted $R/sys/bin/zpod-launcher"
mk; echo "file=$(sum "$R/data/zpod-usb-role") ../../etc/passwd" >> "$R/data/CORE.manifest"; co boot >/dev/null
ok "unsafe path in CORE.manifest -> skipped" "[ \"\$(state)\" = skipped ] && reason | grep -q unsafe"
mk; printf 'not an elf' > "$R/data/bin/zpod-app"; manifest 0.10.0 2000 1; co boot >/dev/null
ok "a non-ARM binary -> skipped" "[ \"\$(state)\" = skipped ] && reason | grep -q 'not an ARM program'"
mk; rm -rf "$R/data/bin" "$R/data/CORE.manifest" "$R/data/zpod_buttons.py"; co boot >/dev/null
ok "no update on the card -> none" "[ \"\$(state)\" = none ]"
mk; rm "$R/share/core.id"
ok "test3 (no core.id): status says no boot support" "co status --kv | grep -qx boot_support=0"
mk; rm "$R/sbin/zpod-usb-role"; co boot >/dev/null
ok "image without zpod-usb-role: the rest still applies" "[ \"\$(state)\" = pending ] && ! mounted $R/sbin/zpod-usb-role && mounted $R/sys/bin/zpod-app"
mk; co off >/dev/null; co apply >/dev/null 2>&1; co off >/dev/null
ok "off: all bind mounts gone" "[ \"\$(nmounts)\" = 0 ] && co status --kv | grep -qx active=image"

rm -rf "$T"
echo "$pass passed, $fail failed"
[ $fail = 0 ]
