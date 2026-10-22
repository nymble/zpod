#!/usr/bin/env python3
"""zpod-battery-guard: clean shutdown before the cell runs flat.

MAX17048 fuel gauge on I2C1 @ 0x36 (confirmed on hardware). Powers off when,
on 3 consecutive reads 30 s apart, ALL of:
  - state of charge (SOC, reg 0x04) < [battery] shutdown_pct (default 5 %)
  - cell voltage (VCELL, reg 0x02) < [battery] shutdown_mv (default 3600 mV)
  - not charging: CRATE (reg 0x16, signed, 0.208 %/h per LSB) <= 0
So a gauge glitch, a fresh cell the gauge has not learned yet, or a ZPOD on
the charger never shuts down. [battery] shutdown_pct = 0 disables the guard.
The launcher shows the threshold (Settings > Battery & about).
"""
import configparser
import subprocess
import sys
import time

try:
    from smbus2 import SMBus
except ImportError:  # pragma: no cover
    SMBus = None

ADDR = 0x36
CONF = "/etc/zpod/ui.conf"
INTERVAL_S = 30
NEEDED = 3


def log(msg):
    print(msg, flush=True)


def conf():
    c = configparser.ConfigParser()
    c.read(CONF)
    b = c["battery"] if c.has_section("battery") else {}
    return int(b.get("shutdown_pct", 5)), int(b.get("shutdown_mv", 3600))


def read(bus):
    def w(reg):
        hi, lo = bus.read_i2c_block_data(ADDR, reg, 2)
        return (hi << 8) | lo
    soc = w(0x04) / 256.0
    mv = w(0x02) * 78.125 / 1000.0
    crate = w(0x16)
    if crate >= 0x8000:
        crate -= 0x10000
    return soc, mv, crate * 0.208


def low(soc, mv, rate, pct, lim_mv):
    return soc < pct and mv < lim_mv and rate <= 0


def main():
    pct, lim_mv = conf()
    if pct <= 0:
        log("zpod-battery-guard: disabled (shutdown_pct = 0)")
        return 0
    if SMBus is None:
        log("zpod-battery-guard: python3-smbus2 missing")
        return 0
    log(f"zpod-battery-guard: power off below {pct}% and {lim_mv} mV while discharging")
    hits = 0
    while True:
        try:
            with SMBus(1) as bus:
                soc, mv, rate = read(bus)
        except OSError as e:
            log(f"zpod-battery-guard: gauge read failed: {e}")
            hits = 0
            time.sleep(INTERVAL_S)
            continue
        if low(soc, mv, rate, pct, lim_mv):
            hits += 1
            log(f"zpod-battery-guard: LOW {hits}/{NEEDED}: {soc:.1f}% {mv:.0f} mV {rate:+.1f}%/h")
            if hits >= NEEDED:
                log("zpod-battery-guard: battery empty, powering off")
                subprocess.run(["systemctl", "poweroff"], check=False)
                return 0
        else:
            hits = 0
        time.sleep(INTERVAL_S)


if __name__ == "__main__":
    sys.exit(main())
