#!/usr/bin/env python3
"""zpod-buttons: ZPOD GPIO buttons -> uinput keyboard ("zpod-buttons").

Reads the confirmed BCM button map (active-low) by polling the GPIO level
register through /dev/gpiomem. Lines are never claimed and pin functions are
never changed, so GPIO20 (side volume down) stays the I2S DIN pin; only its
pull-up is set. Pull-ups are set for every button pin at start.

D-pad centre has no pin of its own: it is reported when GPIO 4, 17, 22 and 23
all go low within COMBO_WINDOW (~40 ms observed). Directions are therefore
emitted after a short hold-off so a centre press never leaks an arrow key.

Side volume buttons: this daemon is the ONLY writer of the DAC's ALSA
"Digital" control. Timing (Howard Qiao's zpod/settings.ini, approved
2026-10-22): a press must last >= 75 ms (shorter = ignored), it then steps
once (2 %); held past 650 ms it repeats one step every 150 ms, steadily,
until released. Volume is capped at 80 %. Each step is computed here from an
integer (no amixer relative steps, no rounding drift, no jumps) and applied
by one writer thread with "latest value wins" (no amixer backlog). Before
each key event the daemon writes /run/zpod/volume = "<pct> <held>" so every
UI shows the same number with no subprocess. Events: KEY_VOLUMEUP/DOWN press
(first step), one autorepeat (value 2) per further step, release on key-up.
Kernel autorepeat is not used for any key; the daemon generates D-pad
repeats itself (400 ms delay, 120 ms period), so events are deterministic.

Optional overrides: /etc/zpod/buttons.conf (INI, see the shipped example).
"""
import configparser
import mmap
import os
import signal
import struct
import subprocess
import sys
import threading
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
VOL_STEP_PCT = 2
VOL_CAP_PCT = 80
KEY_SHORT_MIN_S = 0.075      # shorter presses are ignored
VOL_REPEAT_DELAY_S = 0.650   # key_short_max: held longer = repeat
VOL_REPEAT_S = 0.150
KEY_LONG_MAX_S = 1.200       # long-press actions (documented for apps)
NAV_REPEAT_DELAY_S = 0.400
NAV_REPEAT_S = 0.120
VOL_STATE = "/run/zpod/volume"

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
            "volume_step_pct": VOL_STEP_PCT, "volume_adjusts_alsa": True,
            "volume_cap_pct": VOL_CAP_PCT, "key_short_min_ms": KEY_SHORT_MIN_S * 1000,
            "volume_repeat_delay_ms": VOL_REPEAT_DELAY_S * 1000, "volume_repeat_ms": VOL_REPEAT_S * 1000}
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
            opts["volume_cap_pct"] = max(0, min(100, o.getint("volume_cap_pct", opts["volume_cap_pct"])))
            for k in ("key_short_min_ms", "volume_repeat_delay_ms", "volume_repeat_ms"):
                opts[k] = o.getfloat(k, opts[k])
    codes = {n: e.ecodes[k] for n, k in keys.items()}
    return pins, keys, codes, center, e.ecodes[center], opts


def vol_step(pct, up, step, cap):
    """One volume step from an integer, clamped to [0, cap] (no drift)."""
    p = min(pct, cap)
    return max(0, min(cap, p + step if up else p - step))


def alsa_get():
    try:
        out = subprocess.run(["amixer", "-c", CARD, "sget", "Digital"],
                             capture_output=True, text=True, timeout=3).stdout
    except Exception:
        return None
    import re
    m = re.search(r"\[(\d+)%\]", out)
    return int(m.group(1)) if m else None


class VolumeWriter:
    """Single writer of ALSA 'Digital' (absolute %, latest value wins) and of
    the /run/zpod/volume state file."""

    def __init__(self, enabled):
        self.enabled = enabled
        self.want = None
        self.cv = threading.Condition()
        if enabled:
            threading.Thread(target=self._run, daemon=True).start()

    def publish(self, pct, held):
        try:
            os.makedirs(os.path.dirname(VOL_STATE), exist_ok=True)
            tmp = VOL_STATE + ".tmp"
            with open(tmp, "w") as f:
                f.write(f"{pct} {1 if held else 0}\n")
            os.replace(tmp, VOL_STATE)
        except OSError:
            pass

    def set(self, pct):
        with self.cv:
            self.want = pct
            self.cv.notify()

    def _run(self):
        while True:
            with self.cv:
                while self.want is None:
                    self.cv.wait()
                pct, self.want = self.want, None
            subprocess.run(["amixer", "-q", "-c", CARD, "sset", "Digital", f"{pct}%"],
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
    vcap = opts["volume_cap_pct"]
    short_min = opts["key_short_min_ms"] / 1000.0
    vdelay = opts["volume_repeat_delay_ms"] / 1000.0
    vperiod = opts["volume_repeat_ms"] / 1000.0
    vw = VolumeWriter(opts["volume_adjusts_alsa"])
    vol = alsa_get() if opts["volume_adjusts_alsa"] else None
    if vol is None:
        vol = vcap
    vw.publish(vol, False)

    gm = GpioMem()
    set_pullups(gm, pins.values())
    time.sleep(0.01)

    caps = {e.EV_KEY: sorted(set(codes.values()) | {center_code})}
    ui = evdev.UInput(caps, name="zpod-buttons", vendor=0x5a50, product=0x0d02, version=2)
    # No kernel autorepeat (EV_REP is not in caps): repeats are generated
    # below so held volume keys step steadily and D-pad repeat is uniform.
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
    # volume key state: name -> {"t0": press time, "next": next step time, "on": event sent}
    vol_keys = {"volup": None, "voldown": None}
    nav_next = {}                     # D-pad key -> next autorepeat time

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
                and vol_keys["volup"] is None and vol_keys["voldown"] is None
                and not nav_next):
            time.sleep(POLL_S)
            continue
        last_lev = lev
        now = time.monotonic()
        raw = {n: not (lev >> pins[n]) & 1 for n in pins}   # active-low

        # --- simple buttons ---------------------------------------------
        for n in others:
            if deb[n].update(raw[n], now, debounce):
                if n in vol_keys:
                    if deb[n].state:
                        if all(v is None for v in vol_keys.values()) and opts["volume_adjusts_alsa"]:
                            cur = alsa_get()       # re-sync if something else set it
                            if cur is not None:
                                vol = min(cur, vcap)
                        vol_keys[n] = {"t0": now, "next": now + short_min, "on": False}
                    else:
                        st = vol_keys[n]
                        vol_keys[n] = None
                        held = any(v is not None for v in vol_keys.values())
                        vw.publish(vol, held)
                        if st and st["on"]:
                            emit(codes[n], 0)
                    continue
                emit(codes[n], 1 if deb[n].state else 0)
        for n, st in vol_keys.items():
            if st is None or now < st["next"]:
                continue
            # first step at 75 ms (short_min), repeats from 650 ms every 150 ms
            nv = vol_step(vol, n == "volup", vstep, vcap)
            changed, vol = nv != vol, nv
            vw.publish(vol, True)
            if opts["volume_adjusts_alsa"] and changed:
                vw.set(vol)
            if not st["on"]:
                st["on"] = True
                emit(codes[n], 1)
                st["next"] = st["t0"] + vdelay
            else:
                ui.write(e.EV_KEY, codes[n], 2)
                ui.syn()
                st["next"] = max(st["next"] + vperiod, now + vperiod * 0.5)

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
                        nav_next[n] = now + NAV_REPEAT_DELAY_S
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
                nav_next.clear()
                emit(center_code, 1)
                mode = "center"
            else:
                for n in DPAD:
                    if deb[n].state and n not in dir_down:
                        dir_down.add(n)
                        emit(codes[n], 1)
                        nav_next[n] = now + NAV_REPEAT_DELAY_S
                    elif not deb[n].state and n in dir_down:
                        dir_down.discard(n)
                        nav_next.pop(n, None)
                        emit(codes[n], 0)
                for n, t in list(nav_next.items()):
                    if n in dir_down and now >= t:
                        ui.write(e.EV_KEY, codes[n], 2)
                        ui.syn()
                        nav_next[n] = t + NAV_REPEAT_S
                if not dir_down and deb_none:
                    mode = "idle"

        if mode != "dir" and nav_next:
            nav_next.clear()
        time.sleep(POLL_S)

    ui.close()
    log("zpod-buttons stopped")


if __name__ == "__main__":
    sys.exit(main())
