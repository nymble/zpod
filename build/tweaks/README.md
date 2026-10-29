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

## M2 substep `stage2-05-zpod-m2/` → `stage2/05-zpod-m2`
- `00-packages`: dkms, kernel headers (rpi-v6/v7), python3-pil, python3-evdev,
  python3-lgpio, python3-smbus2, python3-spidev, fonts-dejavu-core, alsa-utils,
  gpiod, i2c-tools, iw, evtest
- `01-run.sh`: installs `overlay/` (zpod-ui, zpod-buttons, configs), the
  `aoide-zpod-dac-dkms` deb (DKMS build per kernel, fails the build if the
  rpi-v6 module is missing), config.txt/cmdline.txt edits, USB gadget NM
  profiles, enables services, and checks Python imports in the chroot.

Only hardware-confirmed pin maps are used (docs/stock-introspection.md).

## M5 substep `stage2-07-zpod-m5/` → `stage2/07-zpod-m5`
- Installs and enables `zpod-data-grow.service` (grows partition 3 to the end
  of the card, max 2 TiB) and `zpod-data-setup.service` (owners of
  `/data/media/{music,playlists}` and `/data/zpod/appdata`); the partition
  itself is created by `scripts/export-image-offset.sh` (see docs/STORAGE.md).
- Installs the optional rsync share (`zpod-rsyncd.socket`, USB link only, no
  password) **disabled**; the File transfer screen turns it on/off.
- Enables `bluetooth.service` (Tools > Bluetooth scanner; the launcher clears
  the rfkill soft block when scanning).
- Size trims: no docs (copyright kept), man pages, info, non-English
  translations (`/etc/dpkg/dpkg.cfg.d/90-zpod-trim` keeps them out of later
  installs too), empty apt lists (`sudo apt update` before installing).
