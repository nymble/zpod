################################################################################
# zpod-scripts: hardware helpers from the repo's pi-gen overlay/
################################################################################
ZPOD_SCRIPTS_VERSION = 1.0
ZPOD_SCRIPTS_SITE = $(BR2_EXTERNAL_ZPOD_PATH)/../overlay
ZPOD_SCRIPTS_SITE_METHOD = local
ZPOD_SCRIPTS_LICENSE = MIT
ZPOD_SCRIPTS_FILES = zpod_buttons.py zpod_battery_guard.py zpod_hw.py \
	zpod-usb-gadget.sh zpod-audio-init.sh

define ZPOD_SCRIPTS_INSTALL_TARGET_CMDS
	$(INSTALL) -d -m 0755 $(TARGET_DIR)/usr/lib/zpod $(TARGET_DIR)/etc/zpod
	for f in $(ZPOD_SCRIPTS_FILES); do \
		$(INSTALL) -m 0755 $(@D)/usr/lib/zpod/$$f $(TARGET_DIR)/usr/lib/zpod/$$f || exit 1; \
	done
	$(INSTALL) -m 0644 $(@D)/etc/asound.conf $(TARGET_DIR)/etc/asound.conf
	$(INSTALL) -m 0644 $(@D)/etc/zpod/buttons.conf $(@D)/etc/zpod/ui.conf $(TARGET_DIR)/etc/zpod/
endef

$(eval $(generic-package))
