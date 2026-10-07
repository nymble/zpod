#!/usr/bin/env bash
# Apply ZPOD M1.1 package lightening + Wi-Fi/i2c tweaks onto a pi-gen checkout.
# Idempotent. Does not run the build.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PI_GEN="${1:-}"
if [[ -z "$PI_GEN" || ! -d "$PI_GEN/stage2" ]]; then
  echo "usage: $0 /path/to/pi-gen" >&2
  exit 2
fi

install -m 644 "$SCRIPT_DIR/stage2-01-sys-tweaks-00-packages" \
  "$PI_GEN/stage2/01-sys-tweaks/00-packages"
install -m 644 "$SCRIPT_DIR/stage2-01-sys-tweaks-00-packages-nr" \
  "$PI_GEN/stage2/01-sys-tweaks/00-packages-nr"
install -m 644 "$SCRIPT_DIR/stage2-02-net-tweaks-00-packages" \
  "$PI_GEN/stage2/02-net-tweaks/00-packages"

# Drop cloud-init package install for lite console (Imager still works via
# firstrun.sh + imager_custom in raspberrypi-sys-mods). Keep the stage dir
# so ENABLE_CLOUD_INIT=1 can be re-enabled by restoring upstream packages.
if [[ -f "$PI_GEN/stage2/04-cloud-init/00-packages" ]]; then
  : > "$PI_GEN/stage2/04-cloud-init/00-packages"
fi

echo "applied ZPOD M1.1 tweaks to $PI_GEN"
