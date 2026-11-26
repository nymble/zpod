#!/bin/sh
# Copyright (C) 2026 Paul Lambert
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
# Build a host-side apps dir for the launcher simulator from packages/ + target/debug.
set -e
here=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-/tmp/zpod-sim/apps}
rm -rf "$out"; mkdir -p "$out"
for a in music clock tetris; do
  mkdir -p "$out/$a/bin"
  cp "$here/packages/$a/manifest.toml" "$here/packages/$a/icon.png" "$out/$a/"
  cp "$here/target/debug/$a" "$out/$a/bin/$a"
done
echo "$out"
