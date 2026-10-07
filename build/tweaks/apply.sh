#!/usr/bin/env bash
# Apply ZPOD M1.1 package lightening + Wi-Fi/i2c tweaks and the M2 substep
# (stage2/05-zpod-m2: DAC DKMS, zpod-ui, zpod-buttons, USB gadget) onto a
# pi-gen checkout.
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

# M2: stage2/05-zpod-m2 = package list + run script + files/{overlay,deb}
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
M2="$PI_GEN/stage2/05-zpod-m2"
rm -rf "$M2"
install -d "$M2/files"
install -m 644 "$SCRIPT_DIR/stage2-05-zpod-m2/00-packages" "$M2/00-packages"
install -m 755 "$SCRIPT_DIR/stage2-05-zpod-m2/01-run.sh" "$M2/01-run.sh"
cp -a "$REPO_ROOT/overlay" "$M2/files/overlay"
find "$M2/files/overlay" -name __pycache__ -prune -exec rm -rf {} +
"$REPO_ROOT/drivers/aoide-zpod-dac/build-deb.sh" "$M2/files" >/dev/null

echo "applied ZPOD M1.1 + M2 tweaks to $PI_GEN"
