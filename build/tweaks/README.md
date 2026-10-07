# pi-gen stage tweaks (M1.1+)

`apply.sh` overlays lightened package lists onto a cloned `build/pi-gen` tree.
`./build.sh --confirm` runs it after clone.

## Adds
- `i2c-tools` (`i2cdetect`)

## Keeps (Wi-Fi / SSH)
- `wpasupplicant`, `wireless-tools`, `firmware-brcm80211`, `network-manager`,
  `raspberrypi-net-mods`, `openssh-server` / `ssh`, `bluez`

## Removes vs stock stage2 (safe for lite console)
- Dev: `build-essential`, `manpages-dev`, `gdb`, `pkg-config`
- Docs noise: `man-db`, `apt-listchanges`
- Camera/video: `rpicam-apps-lite`, `mkvtoolnix`
- Misc fat: `cifs-utils`, `lua5.1`, `luajit`, `rpi-connect-lite`, `pciutils`,
  `p7zip-full`, `kms++-utils`, `rpi-update`, `rpi-eeprom` (Zero has no EEPROM
  update path like Pi 4/5; left out intentionally)
- Non-brcm Wi-Fi firmwares: atheros/libertas/realtek/mediatek/marvell
- `cloud-init` packages (empty `04-cloud-init/00-packages`); set
  `ENABLE_CLOUD_INIT=0` in generated config

Do not invent TFT/DAC overlays here.
