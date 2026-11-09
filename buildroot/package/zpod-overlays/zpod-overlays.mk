################################################################################
# zpod-overlays: compile ZPOD DT overlays with host dtc into the boot partition
################################################################################
ZPOD_OVERLAYS_VERSION = 1.0
ZPOD_OVERLAYS_SITE = $(BR2_EXTERNAL_ZPOD_PATH)/../drivers/aoide-zpod-dac
ZPOD_OVERLAYS_SITE_METHOD = local
ZPOD_OVERLAYS_LICENSE = GPL-2.0
ZPOD_OVERLAYS_DEPENDENCIES = host-dtc rpi-firmware
ZPOD_OVERLAYS_INSTALL_TARGET = NO
ZPOD_OVERLAYS_INSTALL_IMAGES = YES

define ZPOD_OVERLAYS_BUILD_CMDS
	for f in $(@D)/*-overlay.dts; do \
		n=$$(basename $$f -overlay.dts); \
		$(HOST_DIR)/bin/dtc -@ -q -I dts -O dtb -o $(@D)/$$n.dtbo $$f || exit 1; \
	done
endef

define ZPOD_OVERLAYS_INSTALL_IMAGES_CMDS
	mkdir -p $(BINARIES_DIR)/rpi-firmware/overlays
	$(INSTALL) -m 0644 $(@D)/*.dtbo $(BINARIES_DIR)/rpi-firmware/overlays/
endef

$(eval $(generic-package))
