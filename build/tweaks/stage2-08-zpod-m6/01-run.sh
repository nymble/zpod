#!/bin/bash -e
# ZPOD M6 (installed by build/tweaks/apply.sh as stage2/08-zpod-m6; runs after
# 07-zpod-m5):
# - zpod-battery (Rust: battery history + low-battery power-off) replaces the
#   Python zpod-battery-guard
# - drop box / OPDS server: config + units, socket OFF by default
# - boot trims (docs/BOOT-TIME.md). Kept on purpose: SSH over USB (gadget,
#   ICS, sshswitch, host-key regeneration), first-boot files from bootfs
#   (userconfig, zpod-firstuser, zpod-wifi-import), /data grow + setup.
# files/system = apps/system, files/dist = apps/dist.
S="files/system"
BOOTFW="${ROOTFS_DIR}/boot/firmware"
CFG="${BOOTFW}/config.txt"
CMD="${BOOTFW}/cmdline.txt"

# 1. binaries and units
install -m 0755 files/dist/core/usr/lib/zpod/bin/zpod-battery "${ROOTFS_DIR}/usr/lib/zpod/bin/zpod-battery"
for u in zpod-battery.service zpod-drop.service zpod-drop.socket zpod-launcher.service; do
	install -m 0644 "$S/systemd/$u" "${ROOTFS_DIR}/etc/systemd/system/$u"
done
# drop.conf: M6 keys (dropbox/opds off). Stage 06 only installs it when absent,
# so write the M6 version here (no token in it; the repo never has one).
install -m 0644 "$S/etc/zpod/drop.conf" "${ROOTFS_DIR}/etc/zpod/drop.conf"
install -m 0644 "$S/etc/zpod/core-version" "${ROOTFS_DIR}/etc/zpod/core-version"
install -m 0755 "$S/sbin/zpod-data-setup" "${ROOTFS_DIR}/usr/lib/zpod/bin/zpod-data-setup"
install -d -m 0755 "${ROOTFS_DIR}/var/lib/zpod"
# never ship a booted-once marker or a hotspot profile
rm -f "${ROOTFS_DIR}/var/lib/zpod/booted-once" "${ROOTFS_DIR}/var/lib/zpod/boot-notice.last" \
	"${ROOTFS_DIR}/etc/NetworkManager/system-connections/zpod-hotspot.nmconnection"

# 2. boot firmware trims
# auto_initramfs=0: the rpi-v6 kernel has ext4 and the SD host drivers built
# in and root= is a PARTUUID, so the 11 MB initramfs only cost load + unpack
# time. Checked below against modules.builtin; the build fails if not.
KV=$(ls "${ROOTFS_DIR}/lib/modules" | grep -- '-rpi-v6$' | head -1)
for m in kernel/fs/ext4/ext4.ko kernel/drivers/mmc/core/mmc_block.ko kernel/drivers/mmc/host/bcm2835.ko kernel/drivers/mmc/host/bcm2835-mmc.ko; do
	grep -qx "$m" "${ROOTFS_DIR}/lib/modules/$KV/modules.builtin" || { echo "stage2-08: $m not built into $KV; keep the initramfs" >&2; exit 1; }
done
sed -i \
	-e 's/^auto_initramfs=1/auto_initramfs=0/' \
	-e 's/^camera_auto_detect=1/camera_auto_detect=0/' \
	-e 's/^display_auto_detect=1/display_auto_detect=0/' \
	-e 's/^dtoverlay=vc4-kms-v3d/#dtoverlay=vc4-kms-v3d  # ZPOD M6: no HDMI\/KMS (LCD is SPI, driven from userspace)/' \
	-e 's/^max_framebuffers=2/#max_framebuffers=2/' \
	"$CFG"
if ! grep -q 'ZPOD M6' "$CFG"; then
	cat >> "$CFG" <<- 'CFGEOF'
		# --- ZPOD M6: boot time (docs/BOOT-TIME.md) ---
		# no rainbow splash, no boot delay; full clock for the first 30 s of boot
		disable_splash=1
		boot_delay=0
		initial_turbo=30
	CFGEOF
fi
grep -q '^auto_initramfs=0' "$CFG"
! grep -q '^dtoverlay=vc4-kms-v3d' "$CFG"
grep -q '^dtoverlay=aoide-zpod-dac' "$CFG"
grep -q '^dtoverlay=dwc2,dr_mode=peripheral' "$CFG"
# kernel: quiet console (printing to the framebuffer console is slow on ARMv6)
grep -qw quiet "$CMD" || sed -i '1 s/$/ quiet loglevel=3 logo.nologo/' "$CMD"
grep -qw quiet "$CMD"; grep -qw rootwait "$CMD"

on_chroot << 'CHEOF'
set -e
# battery: Rust logger/guard instead of the Python guard
systemctl disable zpod-battery-guard.service 2>/dev/null || true
systemctl enable zpod-battery.service
# drop box + catalog: off until switched on in Tools
systemctl disable zpod-drop.socket zpod-drop.service 2>/dev/null || true
# boot trims: nothing on the ZPOD uses these
#  keyboard-setup: no keyboard; console-setup is masked already (M2)
#  rpi-eeprom-update: the Pi Zero has no boot EEPROM
#  apparmor: no profiles are used on the ZPOD (saves the profile load at sysinit)
#  e2scrub_reap: ext4 online-scrub bookkeeping (no LVM here)
for u in keyboard-setup.service rpi-eeprom-update.service apparmor.service e2scrub_reap.service \
	e2scrub_all.timer; do
	systemctl disable "$u" 2>/dev/null || true
done
systemctl mask keyboard-setup.service rpi-eeprom-update.service
# must stay enabled (SSH over USB, first boot, /data)
for u in ssh.service zpod-usb-gadget.service rpi-usb-gadget-ics.service zpod-firstuser.service \
	zpod-wifi-import.service zpod-data-grow.service zpod-data-setup.service zpod-launcher.service \
	zpod-bootsplash.service zpod-rtc.service zpod-battery.service NetworkManager.service; do
	systemctl is-enabled -q "$u" || { echo "stage2-08: $u is not enabled" >&2; exit 1; }
done
! systemctl is-enabled -q zpod-drop.socket
! systemctl is-enabled -q zpod-battery-guard.service
CHEOF
