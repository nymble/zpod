#!/usr/bin/env python3
"""zpod-buttons: ZPOD GPIO buttons -> uinput keyboard ("zpod-buttons").

Reads the confirmed BCM button map (active-low) by polling the GPIO level
register through /dev/gpiomem. Lines are never claimed and pin functions are
never changed, so GPIO20 (side volume down) stays the I2S DIN pin; only its
pull-up is set. Pull-ups are set for every button pin at start.

D-pad centre has no pin of its own: it is reported when GPIO 4, 17, 22 and 23
all go low within COMBO_WINDOW (~40 ms observed). Directions are therefore
emitted after a short hold-off so a centre press never leaks an arrow key.

Side volume buttons adjust the DAC's ALSA "Digital" control directly and also
emit KEY_VOLUMEUP / KEY_VOLUMEDOWN (apps must not apply them a second time).

Optional overrides: /etc/zpod/buttons.conf (INI, see the shipped example).
"""
import configparser
import mmap
import os
import signal
import struct
import subprocess
import sys
import time

import evdev
from evdev import ecodes as e

CONF = "/etc/zpod/buttons.conf"
CARD = "sndrpiaoidezpod"

# name: (BCM pin, default key)
BUTTONS = {
    "up":        (4,  "KEY_UP"),
    "down":      (22, "KEY_DOWN"),
    "left":      (17, "KEY_LEFT"),
    "right":     (23, "KEY_RIGHT"),
    "home":      (24, "KEY_HOME"),
    "playpause": (6,  "KEY_PLAYPAUSE"),
    "triangle":  (13, "KEY_I"),
    "square":    (12, "KEY_U"),
    "circle":    (26, "KEY_K"),
    "x":         (16, "KEY_J"),
    "volup":     (5,  "KEY_VOLUMEUP"),
    "voldown":   (20, "KEY_VOLUMEDOWN"),
}
CENTER_KEY = "KEY_ENTER"
DPAD = ("up", "down", "left", "right")

POLL_S = 0.005
DEBOUNCE_S = 0.020
COMBO_WINDOW_S = 0.050
VOL_STEP_PCT = 3
VOL_REPEAT_DELAY_S = 0.40
VOL_REPEAT_S = 0.15

GPLEV0 = 0x34
GPPUD = 0x94
GPPUDCLK0 = 0x98


def log(msg):
    print(msg, flush=True)


def soc_family():
    try:
        with open("/proc/device-tree/compatible", "rb") as f:
            compat = f.read().decode(errors="ignore")
    except OSError:
        compat = ""
    for fam in ("bcm2712", "bcm2711", "bcm2837", "bcm2836", "bcm2835"):
        if fam in compat:
            return fam
    return "unknown"


class GpioMem:
    def __init__(self):
        fd = os.open("/dev/gpiomem", os.O_RDWR | os.O_SYNC)
        self.m = mmap.mmap(fd, 4096, mmap.MAP_SHARED, mmap.PROT_READ | mmap.PROT_WRITE)
        os.close(fd)

    def levels(self):
        return struct.unpack_from("<I", self.m, GPLEV0)[0]

    def _w(self, off, val):
        struct.pack_into("<I", self.m, off, val)

    def pullup_legacy(self, mask):
        """BCM2835/6/7 GPPUD sequence: changes pull only, never the function."""
        self._w(GPPUD, 2)            # 2 = pull-up
        time.sleep(0.0002)
        self._w(GPPUDCLK0, mask)
        time.sleep(0.0002)
        self._w(GPPUD, 0)
        self._w(GPPUDCLK0, 0)


def set_pullups(gm, pins):
    fam = soc_family()
    mask = 0
    for p in pins:
        mask |= 1 << p
    if fam in ("bcm2835", "bcm2836", "bcm2837"):
        gm.pullup_legacy(mask)
        log(f"pull-ups set via GPPUD ({fam}) on {sorted(pins)}")
    else:
        # Newer SoCs: pinctrl changes only the pull when no function is given.
        for p in pins:
            subprocess.run(["pinctrl", "set", str(p), "pu"], check=False)
        log(f"pull-ups set via pinctrl ({fam}) on {sorted(pins)}")


def load_config():
    keys = {n: k for n, (_, k) in BUTTONS.items()}
    pins = {n: p for n, (p, _) in BUTTONS.items()}
    center = CENTER_KEY
    opts = {"combo_window_ms": COMBO_WINDOW_S * 1000, "debounce_ms": DEBOUNCE_S * 1000,
            "volume_step_pct": VOL_STEP_PCT, "volume_adjusts_alsa": True}
    cp = configparser.ConfigParser(inline_comment_prefixes=(";", "#"))
    if os.path.exists(CONF):
        cp.read(CONF)
        for n in BUTTONS:
            if cp.has_option("keys", n):
                keys[n] = cp.get("keys", n).strip()
        if cp.has_option("keys", "center"):
            center = cp.get("keys", "center").strip()
        if cp.has_section("options"):
            o = cp["options"]
            opts["combo_window_ms"] = o.getfloat("combo_window_ms", opts["combo_window_ms"])
            opts["debounce_ms"] = o.getfloat("debounce_ms", opts["debounce_ms"])
            opts["volume_step_pct"] = o.getint("volume_step_pct", opts["volume_step_pct"])
            opts["volume_adjusts_alsa"] = o.getboolean("volume_adjusts_alsa", True)
    codes = {n: e.ecodes[k] for n, k in keys.items()}
    return pins, keys, codes, center, e.ecodes[center], opts


def alsa_volume(step_pct):
    sign = "+" if step_pct > 0 else "-"
    subprocess.Popen(["amixer", "-q", "-c", CARD, "sset", "Digital", f"{abs(step_pct)}%{sign}"],
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


class Debounced:
    def __init__(self, pressed=False):
        self.state = pressed
        self.cand = pressed
        self.since = 0.0

    def update(self, raw, now, debounce):
        if raw != self.cand:
            self.cand = raw
            self.since = now
        elif raw != self.state and now - self.since >= debounce:
            self.state = raw
            return True
        return False


def main():
    pins, keys, codes, center_name, center_code, opts = load_config()
    debounce = opts["debounce_ms"] / 1000.0
    window = opts["combo_window_ms"] / 1000.0
    vstep = opts["volume_step_pct"]

    gm = GpioMem()
    set_pullups(gm, pins.values())
    time.sleep(0.01)

    caps = {e.EV_KEY: sorted(set(codes.values()) | {center_code})}
    ui = evdev.UInput(caps, name="zpod-buttons", vendor=0x5a50, product=0x0d02, version=2)
    # Kernel software autorepeat for held keys (arrows etc.)
    try:
        ui.write(e.EV_REP, e.REP_DELAY, 400)
        ui.write(e.EV_REP, e.REP_PERIOD, 120)
    except Exception:
        pass
    log("uinput 'zpod-buttons' ready: " +
        ", ".join(f"{n}=GPIO{pins[n]}->{keys[n]}" for n in pins) +
        f", center=GPIO4+17+22+23->{center_name}")

    def emit(code, val):
        ui.write(e.EV_KEY, code, val)
        ui.syn()

    deb = {n: Debounced() for n in pins}
    others = [n for n in pins if n not in DPAD]
    # D-pad state machine: idle -> pending -> (center | dir) -> idle
    mode = "idle"
    pending_since = 0.0
    last_raw_any = 0.0
    seen = set()
    dir_down = set()
    vol_next = {"volup": None, "voldown": None}

    running = True

    def stop(*_):
        nonlocal running
        running = False
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)

    last_lev = None
    while running:
        lev = gm.levels()
        # Fast path (keeps CPU low on the Zero): nothing changed and nothing
        # is mid-debounce, mid-combo or auto-repeating.
        if (lev == last_lev and mode == "idle"
                and all(d.cand == d.state for d in deb.values())
                and vol_next["volup"] is None and vol_next["voldown"] is None):
            time.sleep(POLL_S)
            continue
        last_lev = lev
        now = time.monotonic()
        raw = {n: not (lev >> pins[n]) & 1 for n in pins}   # active-low

        # --- simple buttons ---------------------------------------------
        for n in others:
            if deb[n].update(raw[n], now, debounce):
                emit(codes[n], 1 if deb[n].state else 0)
                if n in vol_next:
                    if deb[n].state:
                        if opts["volume_adjusts_alsa"]:
                            alsa_volume(vstep if n == "volup" else -vstep)
                        vol_next[n] = now + VOL_REPEAT_DELAY_S
                    else:
                        vol_next[n] = None
        for n, t in vol_next.items():
            if t is not None and now >= t and deb[n].state:
                if opts["volume_adjusts_alsa"]:
                    alsa_volume(vstep if n == "volup" else -vstep)
                vol_next[n] = now + VOL_REPEAT_S

        # --- D-pad + centre combo ----------------------------------------
        for n in DPAD:
            deb[n].update(raw[n], now, debounce)
        raw_any = any(raw[n] for n in DPAD)
        raw_all = all(raw[n] for n in DPAD)
        if raw_any:
            last_raw_any = now
        deb_none = not any(deb[n].state for n in DPAD) and not raw_any

        if mode == "idle":
            if raw_any:
                mode = "pending"
                pending_since = now
                seen.clear()
        elif mode == "pending":
            seen.update(n for n in DPAD if deb[n].state)
            if raw_all:
                emit(center_code, 1)
                mode = "center"
            elif now - pending_since >= window:
                held = [n for n in DPAD if deb[n].state or raw[n]]
                if held:
                    mode = "dir"
                    for n in held:
                        dir_down.add(n)
                        emit(codes[n], 1)
                else:
                    for n in seen:                  # quick tap inside window
                        emit(codes[n], 1); emit(codes[n], 0)
                    mode = "idle"
            elif not raw_any and now - last_raw_any >= debounce:
                for n in seen:                      # quick tap inside window
                    emit(codes[n], 1); emit(codes[n], 0)
                mode = "idle"
        elif mode == "center":
            if deb_none:
                emit(center_code, 0)
                mode = "idle"
        elif mode == "dir":
            if raw_all:
                for n in list(dir_down):
                    emit(codes[n], 0)
                dir_down.clear()
                emit(center_code, 1)
                mode = "center"
            else:
                for n in DPAD:
                    if deb[n].state and n not in dir_down:
                        dir_down.add(n)
                        emit(codes[n], 1)
                    elif not deb[n].state and n in dir_down:
                        dir_down.discard(n)
                        emit(codes[n], 0)
                if not dir_down and deb_none:
                    mode = "idle"

        time.sleep(POLL_S)

    ui.close()
    log("zpod-buttons stopped")


if __name__ == "__main__":
    sys.exit(main())
