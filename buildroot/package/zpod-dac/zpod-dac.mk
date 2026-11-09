################################################################################
# zpod-dac: out-of-tree ASoC machine driver, built from drivers/aoide-zpod-dac
################################################################################
ZPOD_DAC_VERSION = 1.0
ZPOD_DAC_SITE = $(BR2_EXTERNAL_ZPOD_PATH)/../drivers/aoide-zpod-dac
ZPOD_DAC_SITE_METHOD = local
ZPOD_DAC_LICENSE = GPL-2.0
$(eval $(kernel-module))
$(eval $(generic-package))
