#!/bin/sh
# Render the M6 screenshots with the headless simulator (any Linux host).
#   tools/m6-sim.sh -> /tmp/zpod-sim/out/*.png
set -eu
here=$(cd "$(dirname "$0")/.." && pwd); cd "$here"
cargo build -q -p zpod-launcher -p zpod-music -p zpod-clock -p zpod-tetris
tools/sim-apps.sh >/dev/null
. tools/sim/env.sh
rm -rf "$ZPOD_SIM"; mkdir -p "$ZPOD_SIM" /tmp/zpod-sim/data
python3 tools/sim/battlog.py /tmp/zpod-sim/batt
printf 'music_dir = /tmp/zpod-sim/music\n# token =\n' > /tmp/zpod-sim/drop.conf
export ZPOD_SIM_BATTLOG=/tmp/zpod-sim/batt/battery.log ZPOD_SIM_BATTSESS=/tmp/zpod-sim/batt/battery-sessions.log
export ZPOD_DROP_CONF=/tmp/zpod-sim/drop.conf ZPOD_SIM_BATT=58 ZPOD_SIM_CRATE=-9.4 ZPOD_NOTICE=/nonexistent
ZPOD_SIM_SCRIPT=tools/sim/m6.txt target/debug/zpod-launcher
printf '%s\n' "userconf.txt: pi is now paul, password set" "sudo now asks for that password" "zpod-ssh.pub: 1 new SSH key(s) for paul" "zpod-wifi.txt: Wi-Fi network added" > /tmp/zpod-sim/notice
ZPOD_NOTICE=/tmp/zpod-sim/notice ZPOD_SIM_BATT= ZPOD_SIM_SCRIPT=tools/sim/m6-extra.txt sh -c 'unset ZPOD_SIM_BATT; exec target/debug/zpod-launcher'
ls "$ZPOD_SIM"
