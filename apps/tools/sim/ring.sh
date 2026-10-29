#!/bin/sh
# Ring-screen screenshots: seed a timer that ends in 2 s, run the launcher.
set -e
here=$(cd "$(dirname "$0")/../.." && pwd)
cd "$here"
. tools/sim/env.sh
mkdir -p "$ZPOD_DATA_DIR/clock"
printf 'alarm_on=1\nalarm_time=07:30\nalarm_repeat=daily\nsnooze_min=9\ntimer_secs=180\ntimer_end=%s\n' $(( $(date +%s) + 2 )) > "$ZPOD_DATA_DIR/clock/alarm.conf"
ZPOD_SIM_SCRIPT=tools/sim/ring.txt target/debug/zpod-launcher
# Alarm (snoozed alarm due now): Up = snooze again
printf 'alarm_on=1\nalarm_time=07:30\nalarm_repeat=daily\nsnooze_min=9\nsnooze_until=%s\n' $(( $(date +%s) + 2 )) > "$ZPOD_DATA_DIR/clock/alarm.conf"
printf 'wait 2600\nshot launcher-ring-alarm\nkey up\nwait 200\nshot launcher-ring-snoozed\n' > /tmp/zpod-sim/ring-alarm.txt
ZPOD_SIM_SCRIPT=/tmp/zpod-sim/ring-alarm.txt target/debug/zpod-launcher
rm -f "$ZPOD_DATA_DIR/clock/alarm.conf"
