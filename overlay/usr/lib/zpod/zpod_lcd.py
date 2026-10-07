"""ZPOD 320x240 SPI LCD driver (ILI9340-style controller).

Init sequence and pins copied from the hardware-confirmed probe scripts
(zdisp.py candidate 1 + zorient.py, Pi Zero W, 2026-10-07):

  spidev0.0 (SPI0 CE0 = GPIO8), MOSI 10, SCLK 11, no MISO, 32 MHz, mode 0
  DC = GPIO25, no reset pin, GPIO27 driven HIGH (backlight / power enable;
  inferred, not proven)
  SWRESET, 150 ms, SLPOUT, 150 ms, COLMOD 0x55, MADCTL 0x08, INVOFF,
  NORON, DISPON, then MADCTL 0xE8 (MV|MX|MY|BGR) for 320x240 landscape,
  CASET 0..319, RASET 0..239, RAMWR.

GPIO14 (headphone amp mute) is owned by config.txt (gpio=14=op,dl) and is
never touched here.
"""
import mmap
import os
import struct
import subprocess
import time

import lgpio
import spidev
from PIL import Image, ImageChops

WIDTH, HEIGHT = 320, 240
SPI_BUS, SPI_DEV = 0, 0
SPI_HZ = 32_000_000
PIN_DC = 25
PIN_BL = 27
AMP_MUTE = 14          # never touched
MADCTL_LANDSCAPE = 0xE8

# RGB888 -> big-endian RGB565 lookup tables (high byte RRRRRGGG, low GGGBBBBB)
_T_R = [v & 0xF8 for v in range(256)]
_T_GH = [v >> 5 for v in range(256)]
_T_GL = [(v & 0x1C) << 3 for v in range(256)]
_T_B = [v >> 3 for v in range(256)]


def to_rgb565(img):
    """Convert a PIL RGB image to big-endian RGB565 bytes (C speed)."""
    if img.mode != "RGB":
        img = img.convert("RGB")
    r, g, b = img.split()
    hi = ImageChops.add(r.point(_T_R), g.point(_T_GH))
    lo = ImageChops.add(g.point(_T_GL), b.point(_T_B))
    return Image.merge("LA", (hi, lo)).tobytes()


def _open_chip():
    """Open the SoC GPIO chip (label pinctrl-bcm2835 on the Zero W; chip 0)."""
    fallback = None
    for n in range(8):
        try:
            h = lgpio.gpiochip_open(n)
        except Exception:
            continue
        try:
            label = str(lgpio.gpio_get_chip_info(h)[3])
        except Exception:
            label = ""
        if "bcm2835" in label or "bcm2711" in label or "rp1" in label:
            if fallback is not None:
                lgpio.gpiochip_close(fallback)
            return h
        if fallback is None:
            fallback = h
        else:
            lgpio.gpiochip_close(h)
    if fallback is None:
        raise RuntimeError("no GPIO chip found")
    return fallback


class LCD:
    def __init__(self, dc=PIN_DC, bl=PIN_BL, hz=SPI_HZ):
        assert AMP_MUTE not in (dc, bl)
        self.dc = dc
        self.bl = bl
        self.h = _open_chip()
        self.mem = None
        try:
            lgpio.gpio_claim_output(self.h, dc, 1)
        except Exception:
            # Same fallback as zdisp.py: DC owned by pinmux -> pinctrl + gpiomem
            subprocess.run(["pinctrl", "set", str(dc), "op", "dh"], check=True)
            fd = os.open("/dev/gpiomem", os.O_RDWR | os.O_SYNC)
            self.mem = mmap.mmap(fd, 4096, mmap.MAP_SHARED,
                                 mmap.PROT_READ | mmap.PROT_WRITE)
            os.close(fd)
        lgpio.gpio_claim_output(self.h, bl, 1)
        self.spi = spidev.SpiDev()
        self.spi.open(SPI_BUS, SPI_DEV)
        self.spi.max_speed_hz = hz
        self.spi.mode = 0

    # --- low level -------------------------------------------------------
    def _dcw(self, v):
        if self.mem is None:
            lgpio.gpio_write(self.h, self.dc, v)
        else:
            struct.pack_into("<I", self.mem, 0x1C if v else 0x28, 1 << self.dc)

    def cmd(self, c, data=None):
        self._dcw(0)
        self.spi.writebytes([c])
        if data is not None:
            self._dcw(1)
            self.spi.writebytes2(bytes(data))

    # --- panel ------------------------------------------------------------
    def init(self):
        self.cmd(0x01); time.sleep(0.15)      # SWRESET
        self.cmd(0x11); time.sleep(0.15)      # SLPOUT
        self.cmd(0x3A, [0x55])                # COLMOD 16-bit
        self.cmd(0x36, [0x08])                # MADCTL BGR (confirmed init)
        self.cmd(0x20)                        # INVOFF
        self.cmd(0x13)                        # NORON
        self.cmd(0x29); time.sleep(0.05)      # DISPON
        self.cmd(0x36, [MADCTL_LANDSCAPE])    # 0xE8 MV|MX|MY|BGR -> 320x240

    def window(self, x0, y0, x1, y1):
        self.cmd(0x2A, [x0 >> 8, x0 & 255, x1 >> 8, x1 & 255])
        self.cmd(0x2B, [y0 >> 8, y0 & 255, y1 >> 8, y1 & 255])
        self.cmd(0x2C)
        self._dcw(1)

    def show(self, img):
        """Push a full 320x240 PIL image."""
        if img.size != (WIDTH, HEIGHT):
            img = img.resize((WIDTH, HEIGHT))
        buf = to_rgb565(img)
        self.window(0, 0, WIDTH - 1, HEIGHT - 1)
        self.spi.writebytes2(buf)

    def fill(self, rgb565):
        self.window(0, 0, WIDTH - 1, HEIGHT - 1)
        self.spi.writebytes2(bytes([rgb565 >> 8, rgb565 & 255]) * (WIDTH * HEIGHT))

    def display_on(self, on=True):
        self.cmd(0x29 if on else 0x28)       # DISPON / DISPOFF

    def backlight(self, on=True):
        lgpio.gpio_write(self.h, self.bl, 1 if on else 0)

    def close(self):
        try:
            self.spi.close()
        finally:
            lgpio.gpiochip_close(self.h)
