#!/bin/sh
# CI-style portability check: the app cores must build for a bare-metal
# target (no std, alloc only). thumbv7em-none-eabihf stands in for MCUs.
set -e
cd "$(dirname "$0")/.."
T=thumbv7em-none-eabihf
for p in zpod-core zpod-drivers zpod-panel zpod-ui tetris-core; do
  echo "== $p"; cargo build -q --target $T -p $p
done
for p in zpod-clock zpod-music; do
  echo "== $p (--no-default-features)"; cargo build -q --target $T -p $p --no-default-features --lib
done
echo "no_std check OK"
