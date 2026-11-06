#!/bin/sh
# GPS tool states in the simulator: sh gps.sh <out-dir> (from run.sh)
here=$(cd "$(dirname "$0")" && pwd)
apps=$(cd "$here/../../.." && pwd)
out=$1
T=$apps/target/debug
fix=$apps/crates/zpod-nmea/tests/fixtures/nd105c-cold-start.nmea
one() { # state fixture role
	sed "s/STATE/$1/" "$here/gps-one.txt" > /tmp/gps-one-$1.txt
	ZPOD_SIM=$out ZPOD_SIM_SCRIPT=/tmp/gps-one-$1.txt ZPOD_USB_ROLE=$3 ZPOD_GPS_SIM=$2 $T/gps 2>/dev/null || true
}
ZPOD_SIM=$out ZPOD_SIM_SCRIPT=$here/gps-fix.txt ZPOD_USB_ROLE=host ZPOD_GPS_SIM=$fix $T/gps 2>/dev/null || true
one unsupported "$fix" gadget
one no-device "$here/gps-nodevice.nmea" host
one no-data "$here/gps-nodata.nmea" host
one io-error "$here/gps-ioerror.nmea" host
one bad-data "$here/gps-garbage.nmea" host
