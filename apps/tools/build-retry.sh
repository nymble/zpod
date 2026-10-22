#!/bin/sh
# Retry a cargo command (box under memory pressure: rustc gets OOM-killed).
export PATH=$HOME/.local/bin:$HOME/.cargo/bin:$PATH CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-1}
for i in 1 2 3 4 5 6 7 8 9 10 11 12; do
  nice -n 10 "$@" && { echo "BUILD-OK try $i"; exit 0; }
  echo "BUILD-FAIL try $i"; sleep 20
done
exit 1
