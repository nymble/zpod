# stage2-06-zpod-m4 (DRAFT, not merged into apply.sh)

Installs the M4 Rust userland on top of stage2-05-zpod-m2:

* `/usr/lib/zpod/bin/` zpod-launcher, zpod-bootsplash, zpod-drop, zpod-app
* units: zpod-bootsplash.service (sysinit), zpod-launcher.service (replaces
  zpod-ui.service, which is disabled but left installed as a fallback),
  zpod-drop.socket
* `/opt/zpod/apps/{music,clock,tetris}` installed from the .zpa packages
* MPD with `/etc/mpd.conf` -> `/srv/zpod/media/music`, ALSA hw on the PCM5122,
  hardware mixer 'Digital' (the same control zpod-buttons drives)
* system user `zpod` (apps run as it), media dirs owned by `pi`

Prerequisite on the build host: `apps/tools/build-arm.sh` (produces apps/dist).
To wire in: copy this directory like stage2-05 in `build/tweaks/apply.sh` and
make sure apps/dist is reachable from the pi-gen tree (`M4=` at the top).
Rollback on a device: `sudo systemctl disable --now zpod-launcher zpod-bootsplash && sudo systemctl enable --now zpod-ui`.
