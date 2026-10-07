/*
 * ASoC Driver for Aoide ZPOD DAC (PCM5122 @ 0x4d)
 *
 * Out-of-tree rebuild for Raspberry Pi OS 6.x matching vendor overlay
 * compatible "aoide,aoide-zpod-dac" and codec pcm512x.1-004d.
 * Original vendor module: GPL v2, WWW.AOIDE.CC / aoide-zpod-dac.c
 *
 * Licensed under GPL v2.
 */

#include <linux/module.h>
#include <linux/platform_device.h>
#include <linux/of.h>
#include <sound/core.h>
#include <sound/pcm.h>
#include <sound/pcm_params.h>
#include <sound/soc.h>
#include <sound/soc-dai.h>

/* Virtual regmap addresses from sound/soc/codecs/pcm512x.h */
#define PCM512x_VIRT_BASE	0x100
#define PCM512x_PAGE_BASE(n)	(PCM512x_VIRT_BASE + (0x100 * (n)))
#define PCM512x_GPIO_EN		(PCM512x_PAGE_BASE(0) + 8)
#define PCM512x_GPIO_OUTPUT_4	(PCM512x_PAGE_BASE(0) + 83)
#define PCM512x_GPIO_CONTROL_1	(PCM512x_PAGE_BASE(0) + 86)

static bool digital_gain_0db_limit = true;

static int snd_rpi_aoide_zpod_dac_init(struct snd_soc_pcm_runtime *rtd)
{
	struct snd_soc_component *component = snd_soc_rtd_to_codec(rtd, 0)->component;
	struct snd_soc_card *card = rtd->card;
	int ret;

	snd_soc_component_update_bits(component, PCM512x_GPIO_EN, 0x08, 0x08);
	snd_soc_component_update_bits(component, PCM512x_GPIO_OUTPUT_4, 0x0f, 0x02);
	snd_soc_component_update_bits(component, PCM512x_GPIO_CONTROL_1, 0x08, 0x08);

	if (digital_gain_0db_limit) {
		ret = snd_soc_limit_volume(card, "Digital Playback Volume", 207);
		if (ret < 0)
			dev_warn(card->dev, "Failed to set volume limit: %d\n", ret);
	}

	return 0;
}

static int snd_rpi_aoide_zpod_dac_hw_params(struct snd_pcm_substream *substream,
					   struct snd_pcm_hw_params *params)
{
	struct snd_soc_pcm_runtime *rtd = snd_soc_substream_to_rtd(substream);
	int channels = params_channels(params);
	int width = params_width(params);
	int ret;

	width = width <= 16 ? 16 : 32;

	ret = snd_soc_dai_set_bclk_ratio(snd_soc_rtd_to_cpu(rtd, 0), channels * width);
	if (ret)
		return ret;
	return snd_soc_dai_set_bclk_ratio(snd_soc_rtd_to_codec(rtd, 0), channels * width);
}

static int snd_rpi_aoide_zpod_dac_startup(struct snd_pcm_substream *substream)
{
	struct snd_soc_pcm_runtime *rtd = snd_soc_substream_to_rtd(substream);
	struct snd_soc_component *component = snd_soc_rtd_to_codec(rtd, 0)->component;

	snd_soc_component_update_bits(component, PCM512x_GPIO_CONTROL_1, 0x08, 0x08);
	return 0;
}

static void snd_rpi_aoide_zpod_dac_shutdown(struct snd_pcm_substream *substream)
{
	struct snd_soc_pcm_runtime *rtd = snd_soc_substream_to_rtd(substream);
	struct snd_soc_component *component = snd_soc_rtd_to_codec(rtd, 0)->component;

	snd_soc_component_update_bits(component, PCM512x_GPIO_CONTROL_1, 0x08, 0x00);
}

static const struct snd_soc_ops snd_rpi_aoide_zpod_dac_ops = {
	.hw_params = snd_rpi_aoide_zpod_dac_hw_params,
	.startup = snd_rpi_aoide_zpod_dac_startup,
	.shutdown = snd_rpi_aoide_zpod_dac_shutdown,
};

SND_SOC_DAILINK_DEFS(aoide_zpod_dac,
	DAILINK_COMP_ARRAY(COMP_CPU("bcm2708-i2s.0")),
	DAILINK_COMP_ARRAY(COMP_CODEC("pcm512x.1-004d", "pcm512x-hifi")),
	DAILINK_COMP_ARRAY(COMP_PLATFORM("bcm2708-i2s.0")));

static struct snd_soc_dai_link snd_rpi_aoide_zpod_dac_dai[] = {
	{
		.name = "Aoide Zpod DAC",
		.stream_name = "Aoide Zpod DAC HiFi",
		.dai_fmt = SND_SOC_DAIFMT_I2S | SND_SOC_DAIFMT_NB_NF |
			   SND_SOC_DAIFMT_CBC_CFC,
		.ops = &snd_rpi_aoide_zpod_dac_ops,
		.init = snd_rpi_aoide_zpod_dac_init,
		SND_SOC_DAILINK_REG(aoide_zpod_dac),
	},
};

static struct snd_soc_card snd_rpi_aoide_zpod_dac = {
	.name = "snd_rpi_aoide_zpod_dac",
	.driver_name = "AoideZpodDAC",
	.owner = THIS_MODULE,
	.dai_link = snd_rpi_aoide_zpod_dac_dai,
	.num_links = ARRAY_SIZE(snd_rpi_aoide_zpod_dac_dai),
};

static int snd_rpi_aoide_zpod_dac_probe(struct platform_device *pdev)
{
	struct snd_soc_card *card = &snd_rpi_aoide_zpod_dac;
	int ret;

	card->dev = &pdev->dev;

	if (pdev->dev.of_node) {
		struct device_node *i2s_node;
		struct snd_soc_dai_link *dai = &snd_rpi_aoide_zpod_dac_dai[0];

		i2s_node = of_parse_phandle(pdev->dev.of_node, "i2s-controller", 0);
		if (i2s_node) {
			dai->cpus->dai_name = NULL;
			dai->cpus->of_node = i2s_node;
			dai->platforms->name = NULL;
			dai->platforms->of_node = i2s_node;
		}

		digital_gain_0db_limit = !of_property_read_bool(pdev->dev.of_node,
							       "aoide,24db_digital_gain");
	}

	ret = devm_snd_soc_register_card(&pdev->dev, card);
	if (ret && ret != -EPROBE_DEFER)
		dev_err(&pdev->dev, "snd_soc_register_card() failed: %d\n", ret);
	return ret;
}

static const struct of_device_id snd_rpi_aoide_zpod_dac_of_match[] = {
	{ .compatible = "aoide,aoide-zpod-dac", },
	{},
};
MODULE_DEVICE_TABLE(of, snd_rpi_aoide_zpod_dac_of_match);

static struct platform_driver snd_rpi_aoide_zpod_dac_driver = {
	.driver = {
		.name = "snd-rpi-aoide-zpod-dac",
		.owner = THIS_MODULE,
		.of_match_table = snd_rpi_aoide_zpod_dac_of_match,
	},
	.probe = snd_rpi_aoide_zpod_dac_probe,
};

module_platform_driver(snd_rpi_aoide_zpod_dac_driver);

MODULE_AUTHOR("Nymble (6.x OOT rebuild; original WWW.AOIDE.CC)");
MODULE_DESCRIPTION("ASoC Driver for Aoide zpod DAC");
MODULE_LICENSE("GPL v2");
