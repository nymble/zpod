#!/bin/sh
# QEMU smoke test of the Buildroot image (qemu-system-arm -M raspi0, TCG).
# Run inside the build container:  sh buildroot/tests/qemu-boot.sh [setup|boot] [secs]
#   setup  copy sdcard.img to $Q/sd.img (padded to 2 GiB so /data has room to
#          grow) and put test first-boot files on the FAT partition: a throwaway
#          userconf.txt (random password, hash only), zpod-ssh.pub (test key),
#          zpod-wifi.txt (dummy SSID/PSK, checks the import and shred).
#   late   put a second test key (zpod-ssh.pub) on the FAT partition: on the
#          next boot the launcher shows it in the M6 boot notice.
#   boot   boot sd.img; serial log to $Q/boot<N>.log. zpod.debug on the cmdline
#          dumps the boot marks and state after rcS and 20 s later.
# QEMU has no SPI panel, I2C devices, UDC, Wi-Fi or BT, so those parts are
# expected to be missing. The DTB copy disables the PM/watchdog node (not
# emulated) and the Bluetooth node (keeps the PL011 as a tty: ttyAMA1 on this kernel, the console).
set -eu
O=${ZPOD_BR_OUT:-/br/output/images}
Q=${ZPOD_QEMU_DIR:-/br/qemu}
mkdir -p "$Q"
case "${1:-boot}" in
setup)
	dtc -q -I dtb -O dts -o "$Q/z.dts" "$O/bcm2708-rpi-zero-w.dtb"
	awk '/watchdog@7e100000 \{/ {print; print "\t\t\tstatus = \"disabled\";"; next}
	     /compatible = "brcm,bcm43438-bt";/ {bt=1}
	     bt && /status = "okay";/ {sub(/"okay"/, "\"disabled\""); bt=0}
	     {print}' "$Q/z.dts" > "$Q/zq.dts"
	dtc -q -I dts -O dtb -o "$Q/zq.dtb" "$Q/zq.dts"
	cp "$O/sdcard.img" "$Q/sd.img"; truncate -s 2G "$Q/sd.img"
	off=$(( $(sfdisk -d "$Q/sd.img" | sed -n 's/.*: start= *\([0-9]*\),.*/\1/p' | head -n1) * 512 ))
	T=$(mktemp -d)
	pw=$(head -c 12 /dev/urandom | od -An -tx1 | tr -d ' \n')
	printf 'paul:%s\n' "$(openssl passwd -6 "$pw")" > "$T/userconf.txt"; pw=
	echo 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIQemuTestKeyNotARealKey000000000000000000 qemu-test' > "$T/zpod-ssh.pub"
	printf 'SSID=%s\nPSK=%s\n' 'qemu-test-net' 'not-a-real-password' > "$T/zpod-wifi.txt"
	for f in userconf.txt zpod-ssh.pub zpod-wifi.txt; do mcopy -o -i "$Q/sd.img@@$off" "$T/$f" ::$f; done
	rm -rf "$T"; mdir -i "$Q/sd.img@@$off" ::
	;;
late)
	off=$(( $(sfdisk -d "$Q/sd.img" | sed -n 's/.*: start= *\([0-9]*\),.*/\1/p' | head -n1) * 512 ))
	T=$(mktemp -d)
	echo 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIQemuLateKeyNotARealKey000000000000000000 qemu-late' > "$T/zpod-ssh.pub"
	mcopy -o -i "$Q/sd.img@@$off" "$T/zpod-ssh.pub" ::zpod-ssh.pub; rm -rf "$T"
	;;
boot)
	n=1; while [ -e "$Q/boot$n.log" ]; do n=$((n+1)); done
	CMD=$(sed 's/console=tty1/console=ttyAMA1/' "$O/rpi-firmware/cmdline.txt")
	timeout "${2:-150}" qemu-system-arm -M raspi0 -m 512 -kernel "$O/zImage" -dtb "$Q/zq.dtb" \
		-drive file="$Q/sd.img",if=sd,format=raw -append "$CMD zpod.debug" \
		-nographic -monitor none -serial stdio > "$Q/boot$n.log" 2>&1 || true
	echo "$Q/boot$n.log"
	;;
esac
