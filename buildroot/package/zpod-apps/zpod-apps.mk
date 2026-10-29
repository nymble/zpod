################################################################################
# zpod-apps: install the prebuilt Rust userland (apps/dist from build-arm.sh)
#
# The binaries are static musl (arm-unknown-linux-musleabihf, built with
# cargo-zigbuild), so they do not link against the Buildroot glibc and are
# byte-identical to the ones in the pi-gen image. Building them through the
# Buildroot cargo infrastructure is a later step (docs/BUILDROOT.md).
################################################################################
ZPOD_APPS_VERSION = $(shell cat $(BR2_EXTERNAL_ZPOD_PATH)/../apps/system/etc/zpod/core-version 2>/dev/null || echo 0)
ZPOD_APPS_SITE = $(BR2_EXTERNAL_ZPOD_PATH)/../apps/system
ZPOD_APPS_SITE_METHOD = local
ZPOD_APPS_LICENSE = MIT
ZPOD_APPS_DIST = $(call qstrip,$(BR2_PACKAGE_ZPOD_APPS_DIST))

define ZPOD_APPS_BUILD_CMDS
	test -x $(ZPOD_APPS_DIST)/core/usr/lib/zpod/bin/zpod-launcher || \
		{ echo "zpod-apps: $(ZPOD_APPS_DIST) missing; run apps/tools/build-arm.sh"; exit 1; }
endef

define ZPOD_APPS_INSTALL_TARGET_CMDS
	$(INSTALL) -d -m 0755 $(TARGET_DIR)/usr/lib/zpod/bin $(TARGET_DIR)/usr/share/zpod/packages
	$(INSTALL) -m 0755 $(ZPOD_APPS_DIST)/core/usr/lib/zpod/bin/* $(TARGET_DIR)/usr/lib/zpod/bin/
	$(INSTALL) -m 0644 $(ZPOD_APPS_DIST)/packages/*.zpa $(TARGET_DIR)/usr/share/zpod/packages/
	$(INSTALL) -D -m 0644 $(@D)/etc/zpod/core-version $(TARGET_DIR)/etc/zpod/core-version
	$(INSTALL) -D -m 0644 $(@D)/etc/zpod/drop.conf $(TARGET_DIR)/etc/zpod/drop.conf
	mkdir -p $(TARGET_DIR)/usr/local/bin
	ln -sf /usr/lib/zpod/bin/zpod-app $(TARGET_DIR)/usr/local/bin/zpod-app
	ln -sf /usr/lib/zpod/bin/zpod-bootsplash $(TARGET_DIR)/usr/local/bin/zpod-bootsplash
endef

$(eval $(generic-package))
