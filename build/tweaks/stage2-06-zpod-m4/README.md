# stage2-06-zpod-m4

Installs the M4 Rust userland on top of stage2-05-zpod-m2:

* `/usr/lib/zpod/bin/` zpod-launcher, zpod-bootsplash, zpod-drop, zpod-app
* units: zpod-bootsplash.service (sysinit), zpod-launcher.service (replaces
  zpod-ui.service, which is disabled but left installed as a fallback),
  zpod-drop.socket
* `/opt/zpod/apps/{music,clock,tetris}` installed from the .zpa packages
* MPD with `/etc/mpd.conf` -> `/srv/zpod/media/music`, ALSA hw on the PCM5122,
  no MPD mixer (mixer_type none): zpod-buttons is the only volume writer
* system user `zpod` (apps run as it), media dirs owned by uid 1000 (the first user, `pi` renamed at first boot)

Prerequisite on the build host: `apps/tools/build-arm.sh` (produces apps/dist).
`build/tweaks/apply.sh` installs this stage (with files/dist and
files/system) only when apps/dist exists.
Rollback on a device: `sudo systemctl disable --now zpod-launcher zpod-bootsplash && sudo systemctl enable --now zpod-ui`.
