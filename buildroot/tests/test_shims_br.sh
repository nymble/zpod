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
status) printf 'bssid=e0:63:da:00:00:01\nssid=HomeNet\nid=0\nwpa_state=COMPLETED\n' ;;
ping) echo PONG ;;
select_network|enable_network) echo OK ;;
all) echo OK ;;
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
ok "nmcli other -> exit 1" "! PATH=$T:\$PATH sh $OV/usr/bin/nmcli general status 2>/dev/null"
# saved networks from wpa_supplicant.conf (quoted and hex SSIDs)
cat > $T/wpa.conf <<'C'
ctrl_interface=DIR=/run/wpa_supplicant GROUP=zpod
network={
	ssid="HomeNet"
	psk=0000000000000000000000000000000000000000000000000000000000000000
}
network={
	ssid=436f666665653a53686f70
	key_mgmt=NONE
}
C
for b in udhcpc ntpd udhcpd ip; do printf '#!/bin/sh\nexit 0\n' > $T/$b; chmod +x $T/$b; done
printf '#!/bin/sh\nexit 1\n' > $T/pidof; chmod +x $T/pidof
printf '#!/bin/sh\necho "S40 $*" >> %s/s40.log\n' $T > $T/S40; chmod +x $T/S40
export ZPOD_WPA_CONF=$T/wpa.conf ZPOD_NM_CONN_DIR=$T/nm ZPOD_RUN=$T/run ZPOD_S40=$T/S40
N="env PATH=$T:$PATH sh $OV/usr/bin/nmcli"
out=$($N -t -f NAME,UUID,TYPE,AUTOCONNECT,ACTIVE connection show)
ok "nmcli saved: current network" "printf '%s\n' \"\$out\" | grep -qx 'HomeNet:wpa-0:802-11-wireless:yes:yes'"
ok "nmcli saved: hex ssid decoded + escaped" "printf '%s\n' \"\$out\" | grep -qxF 'Coffee\\:Shop:wpa-1:802-11-wireless:yes:no'"
ok "nmcli active" "[ \"\$($N -t -f NAME connection show --active)\" = HomeNet ]"
ok "nmcli up uuid wpa-0" "$N --wait 2 connection up uuid wpa-0 >/dev/null"
ok "nmcli up unknown uuid fails" "! $N --wait 1 connection up uuid wpa-7 2>/dev/null"
# hotspot from the launcher's keyfile -> hostapd.conf (0600), AP mode
mkdir -p $T/nm $T/run
printf '[connection]\nid=zpod-hotspot\n[wifi]\nmode=ap\nssid=ZPOD-1a2b\nchannel=11\n[wifi-security]\npsk=test-pass-not-real\n' > $T/nm/zpod-hotspot.nmconnection
cat > $T/hostapd <<H
#!/bin/sh
sleep 30 </dev/null >/dev/null 2>&1 &
echo \$! > \$3
H
chmod +x $T/hostapd
printf '#!/bin/sh\necho "	type AP"\n' > $T/iw; chmod +x $T/iw
ok "hotspot up" "$N --wait 3 connection up zpod-hotspot >/dev/null"
ok "hostapd.conf ssid/channel/wpa2" "grep -qx ssid=ZPOD-1a2b $T/run/hostapd.conf && grep -qx channel=11 $T/run/hostapd.conf && grep -qx wpa=2 $T/run/hostapd.conf"
ok "hostapd.conf is 0600" "ls -l $T/run/hostapd.conf | grep -q '^-rw-------'"
ok "client stopped for hotspot" "grep -q 'S40 stop' $T/s40.log"
ok "hotspot listed active" "$N -t -f NAME connection show --active | grep -qx zpod-hotspot"
ok "hotspot down" "$N connection down zpod-hotspot >/dev/null && [ ! -e $T/run/hostapd.pid ]"
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
