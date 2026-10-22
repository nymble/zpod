#!/bin/sh
# ZPOD: DS3231 real-time clock on I2C1 @ 0x68 (confirmed on hardware with
# i2cdetect; driver: dtoverlay=i2c-rtc,ds3231 -> rtc-ds1307 module, /dev/rtc0).
# The driver is a module, so the kernel's hctosys does not run: do it here.
#   start : system clock <- RTC (if the RTC holds a sane time)
#   sync  : RTC <- system clock, only if NTP has synchronised the clock
#   stop  : same as sync (at shutdown)
# Setting the time by hand (launcher Settings > Date & time) runs
# `date -s` + `hwclock -w` itself.
RTC=/dev/rtc0
i=0
while [ ! -e "$RTC" ] && [ $i -lt 50 ]; do sleep 0.2; i=$((i + 1)); done
[ -e "$RTC" ] || { echo "zpod-rtc: no $RTC (DS3231 missing?)"; exit 0; }
case "${1:-start}" in
start)
	t=$(hwclock -r --utc 2>/dev/null) || { echo "zpod-rtc: RTC unreadable"; exit 0; }
	y=$(echo "$t" | cut -c1-4)
	if [ "${y:-0}" -ge 2025 ] 2>/dev/null; then
		hwclock -s --utc && echo "zpod-rtc: system clock set from RTC ($t)"
	else
		echo "zpod-rtc: RTC time not set yet ($t); waiting for NTP or Settings > Date & time"
	fi ;;
sync|stop)
	if [ "$(timedatectl show -p NTPSynchronized --value 2>/dev/null)" = yes ]; then
		hwclock -w --utc && echo "zpod-rtc: RTC set from NTP-synchronised clock"
	else
		echo "zpod-rtc: clock not NTP-synchronised; RTC left as is"
	fi ;;
esac
