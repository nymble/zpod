#!/bin/sh
# Tests for the Buildroot nmcli shim (wpa_cli backend) and zpod-data-grow
# (dry run on a fake sysfs). Linux container; never touches real devices.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
OV=$HERE/../board/zpod/rootfs-overlay
pass=0 fail=0
ok() { if eval "$2"; then pass=$((pass+1)); echo "ok   $1"; else fail=$((fail+1)); echo "FAIL $1"; fi; }
T=$(mktemp -d)
# fake wpa_cli: status + scan_results (tab separated, header line first)
cat > $T/wpa_cli <<'W'
#!/bin/sh
for a; do last=$a; done
case "$last" in
status) printf 'bssid=e0:63:da:00:00:01\nssid=HomeNet\nwpa_state=COMPLETED\n' ;;
scan) echo OK ;;
scan_results) printf 'bssid / frequency / signal level / flags / ssid\n'
	printf 'e0:63:da:00:00:01\t2462\t-48\t[WPA2-PSK-CCMP][ESS]\tHomeNet\n'
	printf '74:83:c2:00:00:03\t2437\t-67\t[ESS]\tCoffee:Shop\n' ;;
esac
W
chmod +x $T/wpa_cli
out=$(PATH=$T:$PATH sh $OV/usr/bin/nmcli -t -f ACTIVE,SSID,SIGNAL device wifi list --rescan no)
ok "nmcli list: active network" "printf '%s\n' \"\$out\" | grep -qx 'yes:HomeNet:100'"
ok "nmcli list: escaped colon, signal %" "printf '%s\n' \"\$out\" | grep -qxF 'no:Coffee\\:Shop:66'"
ok "nmcli rescan" "PATH=$T:\$PATH sh $OV/usr/bin/nmcli device wifi rescan ifname wlan0"
ok "nmcli other -> exit 1" "! PATH=$T:\$PATH sh $OV/usr/bin/nmcli connection show 2>/dev/null"
# zpod-data-grow dry run: 32 GB card, p4 = 256 MiB at sector 1310720
S=$T/sys; mkdir -p $S/mmcblk0p4
echo 62521344 > $S/size; echo 1310720 > $S/mmcblk0p4/start; echo 524288 > $S/mmcblk0p4/size
out=$(ZPOD_DRY_RUN=1 ZPOD_SYS_BLOCK=$S sh $OV/usr/sbin/zpod-data-grow)
ok "grow: 32 GB card -> to the last sector" "echo \"\$out\" | grep -q 'end sector 1835007 -> 62521343'"
echo $((62521343 - 1310720 + 1)) > $S/mmcblk0p4/size
out=$(ZPOD_DRY_RUN=1 ZPOD_SYS_BLOCK=$S sh $OV/usr/sbin/zpod-data-grow)
ok "grow: already full -> nothing" "[ -z \"\$out\" ]"
echo 8589934592 > $S/size; echo 524288 > $S/mmcblk0p4/size
out=$(ZPOD_DRY_RUN=1 ZPOD_SYS_BLOCK=$S sh $OV/usr/sbin/zpod-data-grow)
ok "grow: 4 TB card stops at the MBR 2 TiB limit" "echo \"\$out\" | grep -q -- '-> 4294967295'"
rm -rf $T
echo "passed $pass, failed $fail"
[ $fail = 0 ]
