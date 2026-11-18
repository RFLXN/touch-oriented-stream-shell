// SPDX-License-Identifier: GPL-2.0
/*
 * 토스 프론트 1.5세대(SUNMI p681x)의 7인치 800x1280 MIPI DSI 패널, 변종 M13.
 * 구동 칩은 ILI9881C 계열(초기화가 FF 98 81로 시작).
 *
 * 순정 커널은 디바이스 트리에 적힌 초기화 명령열을 그대로 보내는 방식이었다.
 * 이 드라이버도 그 명령열(panel-sunmi-toss-front-init.h)을 같은 순서와 지연으로 재생한다.
 * 타이밍과 전원 순서도 순정 DT의 mipi_lcd_23 노드 값이다.
 */

#include <linux/delay.h>
#include <linux/gpio/consumer.h>
#include <linux/mod_devicetable.h>
#include <linux/module.h>
#include <linux/regulator/consumer.h>

#include <drm/drm_mipi_dsi.h>
#include <drm/drm_modes.h>
#include <drm/drm_panel.h>

#include "panel-sunmi-toss-front-init.h"

struct tf_panel {
	struct drm_panel panel;
	struct mipi_dsi_device *dsi;
	struct regulator *power;
	struct regulator *vsp;
	struct regulator *vsn;
	struct gpio_desc *enable;
	struct gpio_desc *vddio;
	struct gpio_desc *reset;
	enum drm_panel_orientation orientation;
};

static inline struct tf_panel *to_tf_panel(struct drm_panel *panel)
{
	return container_of(panel, struct tf_panel, panel);
}

static int tf_panel_send_seq(struct tf_panel *ctx, const u8 *seq, size_t len)
{
	struct mipi_dsi_device *dsi = ctx->dsi;
	size_t i = 0;
	ssize_t ret;

	while (i + 3 <= len) {
		u8 type = seq[i], delay = seq[i + 1], n = seq[i + 2];
		const u8 *data = &seq[i + 3];

		if (i + 3 + n > len)
			return -EINVAL;

		switch (type) {
		case 0x05:
		case 0x15:
		case 0x39:
			ret = mipi_dsi_dcs_write_buffer(dsi, data, n);
			break;
		case 0x03:
		case 0x13:
		case 0x23:
		case 0x29:
			ret = mipi_dsi_generic_write(dsi, data, n);
			break;
		default:
			return -EINVAL;
		}
		if (ret < 0) {
			dev_err(&dsi->dev, "명령 0x%02x 전송 실패(오프셋 %zu): %zd\n",
				n ? data[0] : 0, i, ret);
			return ret;
		}
		if (delay)
			msleep(delay);
		i += 3 + n;
	}

	return 0;
}

static int tf_panel_prepare(struct drm_panel *panel)
{
	struct tf_panel *ctx = to_tf_panel(panel);
	int ret;

	ret = regulator_enable(ctx->power);
	if (ret)
		return ret;

	ret = regulator_enable(ctx->vsp);
	if (ret)
		goto err_power;
	ret = regulator_enable(ctx->vsn);
	if (ret)
		goto err_vsp;
	msleep(10);

	gpiod_set_value_cansleep(ctx->vddio, 1);
	gpiod_set_value_cansleep(ctx->enable, 1);
	msleep(20);

	gpiod_set_value_cansleep(ctx->reset, 1);
	msleep(20);
	gpiod_set_value_cansleep(ctx->reset, 0);
	msleep(150);

	ret = tf_panel_send_seq(ctx, tf_panel_init_seq, sizeof(tf_panel_init_seq));
	if (ret) {
		gpiod_set_value_cansleep(ctx->reset, 1);
		gpiod_set_value_cansleep(ctx->enable, 0);
		gpiod_set_value_cansleep(ctx->vddio, 0);
		regulator_disable(ctx->vsn);
		goto err_vsp;
	}

	msleep(30);

	{
		u8 mode = 0;

		if (!mipi_dsi_dcs_get_power_mode(ctx->dsi, &mode) && mode != 0x9c)
			dev_warn(&ctx->dsi->dev,
				 "패널 전원 상태 0x%02x(기대값 0x9c) — 바이어스 전원 확인 필요\n", mode);
	}

	return 0;

err_vsp:
	regulator_disable(ctx->vsp);
err_power:
	regulator_disable(ctx->power);
	return ret;
}

static int tf_panel_unprepare(struct drm_panel *panel)
{
	struct tf_panel *ctx = to_tf_panel(panel);

	tf_panel_send_seq(ctx, tf_panel_exit_seq, sizeof(tf_panel_exit_seq));
	msleep(10);

	gpiod_set_value_cansleep(ctx->reset, 1);
	gpiod_set_value_cansleep(ctx->enable, 0);
	gpiod_set_value_cansleep(ctx->vddio, 0);
	regulator_disable(ctx->vsn);
	regulator_disable(ctx->vsp);
	regulator_disable(ctx->power);

	return 0;
}

static const struct drm_display_mode tf_panel_mode = {
	.clock = 70224,
	.hdisplay = 800,
	.hsync_start = 800 + 40,
	.hsync_end = 800 + 40 + 20,
	.htotal = 800 + 40 + 20 + 20,
	.vdisplay = 1280,
	.vsync_start = 1280 + 30,
	.vsync_end = 1280 + 30 + 4,
	.vtotal = 1280 + 30 + 4 + 16,
	.flags = DRM_MODE_FLAG_NHSYNC | DRM_MODE_FLAG_NVSYNC,
	.width_mm = 94,
	.height_mm = 151,
	.type = DRM_MODE_TYPE_DRIVER | DRM_MODE_TYPE_PREFERRED,
};

static int tf_panel_get_modes(struct drm_panel *panel, struct drm_connector *connector)
{
	struct tf_panel *ctx = to_tf_panel(panel);
	struct drm_display_mode *mode;

	mode = drm_mode_duplicate(connector->dev, &tf_panel_mode);
	if (!mode)
		return -ENOMEM;

	drm_mode_set_name(mode);
	drm_mode_probed_add(connector, mode);

	connector->display_info.width_mm = mode->width_mm;
	connector->display_info.height_mm = mode->height_mm;
	drm_connector_set_panel_orientation(connector, ctx->orientation);

	return 1;
}

static enum drm_panel_orientation tf_panel_get_orientation(struct drm_panel *panel)
{
	return to_tf_panel(panel)->orientation;
}

static const struct drm_panel_funcs tf_panel_funcs = {
	.prepare = tf_panel_prepare,
	.unprepare = tf_panel_unprepare,
	.get_modes = tf_panel_get_modes,
	.get_orientation = tf_panel_get_orientation,
};

static int tf_panel_probe(struct mipi_dsi_device *dsi)
{
	struct device *dev = &dsi->dev;
	struct tf_panel *ctx;
	int ret;

	ctx = devm_drm_panel_alloc(dev, struct tf_panel, panel, &tf_panel_funcs,
				   DRM_MODE_CONNECTOR_DSI);
	if (IS_ERR(ctx))
		return PTR_ERR(ctx);

	ctx->dsi = dsi;
	mipi_dsi_set_drvdata(dsi, ctx);

	ctx->power = devm_regulator_get(dev, "power");
	if (IS_ERR(ctx->power))
		return dev_err_probe(dev, PTR_ERR(ctx->power), "power 레귤레이터 획득 실패\n");

	ctx->vsp = devm_regulator_get(dev, "vsp");
	if (IS_ERR(ctx->vsp))
		return dev_err_probe(dev, PTR_ERR(ctx->vsp), "vsp 레귤레이터 획득 실패\n");

	ctx->vsn = devm_regulator_get(dev, "vsn");
	if (IS_ERR(ctx->vsn))
		return dev_err_probe(dev, PTR_ERR(ctx->vsn), "vsn 레귤레이터 획득 실패\n");

	ctx->reset = devm_gpiod_get(dev, "reset", GPIOD_OUT_HIGH);
	if (IS_ERR(ctx->reset))
		return dev_err_probe(dev, PTR_ERR(ctx->reset), "reset GPIO 획득 실패\n");

	ctx->enable = devm_gpiod_get_optional(dev, "enable", GPIOD_OUT_LOW);
	if (IS_ERR(ctx->enable))
		return dev_err_probe(dev, PTR_ERR(ctx->enable), "enable GPIO 획득 실패\n");

	ctx->vddio = devm_gpiod_get_optional(dev, "vddio", GPIOD_OUT_LOW);
	if (IS_ERR(ctx->vddio))
		return dev_err_probe(dev, PTR_ERR(ctx->vddio), "vddio GPIO 획득 실패\n");

	ret = of_drm_get_panel_orientation(dev->of_node, &ctx->orientation);
	if (ret)
		return dev_err_probe(dev, ret, "rotation 속성 파싱 실패\n");

	ret = drm_panel_of_backlight(&ctx->panel);
	if (ret)
		return dev_err_probe(dev, ret, "백라이트 등록 실패\n");

	ctx->panel.prepare_prev_first = true;

	drm_panel_add(&ctx->panel);

	dsi->lanes = 3;
	dsi->format = MIPI_DSI_FMT_RGB888;
	dsi->mode_flags = MIPI_DSI_MODE_VIDEO | MIPI_DSI_MODE_VIDEO_BURST |
			  MIPI_DSI_MODE_NO_EOT_PACKET | MIPI_DSI_MODE_LPM;

	ret = mipi_dsi_attach(dsi);
	if (ret) {
		drm_panel_remove(&ctx->panel);
		return dev_err_probe(dev, ret, "DSI 호스트 연결 실패\n");
	}

	return 0;
}

static void tf_panel_remove(struct mipi_dsi_device *dsi)
{
	struct tf_panel *ctx = mipi_dsi_get_drvdata(dsi);

	mipi_dsi_detach(dsi);
	drm_panel_remove(&ctx->panel);
}

static const struct of_device_id tf_panel_of_match[] = {
	{ .compatible = "sunmi,toss-front-m13" },
	{ }
};
MODULE_DEVICE_TABLE(of, tf_panel_of_match);

static struct mipi_dsi_driver tf_panel_driver = {
	.probe = tf_panel_probe,
	.remove = tf_panel_remove,
	.driver = {
		.name = "panel-sunmi-toss-front",
		.of_match_table = tf_panel_of_match,
	},
};
module_mipi_dsi_driver(tf_panel_driver);

MODULE_DESCRIPTION("SUNMI Toss Front M13 800x1280 DSI panel");
MODULE_LICENSE("GPL");
