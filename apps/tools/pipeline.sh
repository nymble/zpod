#!/bin/sh
# Run the M4 build/test steps, waiting for free memory before each (shared,
# memory-starved box). Logs to /tmp/m4-pipe/<step>.log.
export PATH=$HOME/.local/bin:$HOME/.cargo/bin:$PATH CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2}
unset CARGO_PROFILE_DEV_DEBUG
cd "$(dirname "$0")/.."
mkdir -p /tmp/m4-pipe
memok() { awk '/MemAvailable/ {exit !($2 > 2500000)}' /proc/meminfo; }
step() { name=$1; shift
  for i in $(seq 1 40); do
    while ! memok; do sleep 15; done
    echo "$(date +%T) $name try $i" >> /tmp/m4-pipe/status
    if nice -n 10 "$@" > /tmp/m4-pipe/$name.log 2>&1; then echo "$(date +%T) $name OK" >> /tmp/m4-pipe/status; return 0; fi
    grep -q -E "SIGKILL|Killed|signal: 9" /tmp/m4-pipe/$name.log || { echo "$(date +%T) $name FAILED (real error)" >> /tmp/m4-pipe/status; return 1; }
    sleep 20
  done; return 1; }
for s in "$@"; do
  case $s in
    debug) step debug cargo build --workspace ;;
    test) step test cargo test --workspace ;;
    nostd) step nostd tools/check-nostd.sh ;;
    arm) step arm tools/build-arm.sh ;;
    armtest) step armtest cargo-zigbuild test --target arm-unknown-linux-musleabihf -p zpod-core -p zpod-drivers -p zpod-panel -p zpod-ui -p tetris-core -p zpod-apps -p zpod-mpd ;;
  esac
done
echo "$(date +%T) pipeline done" >> /tmp/m4-pipe/status
