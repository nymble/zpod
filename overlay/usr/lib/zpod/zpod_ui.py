#!/usr/bin/env python3
"""zpod-ui: status + menu screen on the ZPOD 320x240 SPI LCD.

Input comes from the uinput keyboard published by zpod-buttons (device name
"zpod-buttons"); the UI never touches button GPIOs itself.

  UP/DOWN        move selection        ENTER (centre) / RIGHT / Circle(K)  select
  LEFT / X(J)    back / cancel         HOME           back to main screen
  VOLUMEUP/DOWN  (already applied to ALSA by zpod-buttons) -> volume overlay

Menu: Status, Play test tone, Volume, Wi-Fi info, Reboot, Shutdown
(Reboot / Shutdown ask for confirmation).
"""
import os
import select
import signal
import subprocess
import sys
import threading
import time

from PIL import Image, ImageDraw, ImageFont

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import zpod_hw as hw            # noqa: E402
from zpod_lcd import LCD, WIDTH, HEIGHT   # noqa: E402

try:
    import evdev
    from evdev import ecodes as e
except Exception:               # UI still shows status without buttons
    evdev = None
    e = None

FONT_DIR = "/usr/share/fonts/truetype/dejavu"
REFRESH_S = 5.0
BG = (0, 0, 24)
FG = (235, 235, 235)
DIM = (140, 140, 160)
ACCENT = (255, 210, 0)
SEL_BG = (0, 90, 160)
OK = (0, 220, 120)
WARN = (255, 80, 60)

MENU = ["Status", "Play test tone", "Volume", "Wi-Fi info", "Reboot", "Shutdown"]


def font(size, bold=False):
    name = "DejaVuSans-Bold.ttf" if bold else "DejaVuSans.ttf"
    try:
        return ImageFont.truetype(os.path.join(FONT_DIR, name), size)
    except Exception:
        return ImageFont.load_default()


F_TITLE = font(22, True)
F_ITEM = font(19)
F_SMALL = font(15)
F_BIG = font(30, True)


def log(msg):
    print(msg, flush=True)


class Buttons:
    """Reads key events from the zpod-buttons uinput device (reconnecting)."""

    def __init__(self):
        self.dev = None
        self.next_try = 0.0

    def fileno(self):
        return self.dev.fd if self.dev else None

    def ensure(self):
        if self.dev or evdev is None or time.monotonic() < self.next_try:
            return
        self.next_try = time.monotonic() + 2.0
        for path in evdev.list_devices():
            try:
                d = evdev.InputDevice(path)
            except OSError:
                continue
            if d.name == "zpod-buttons":
                self.dev = d
                log(f"input: {path} (zpod-buttons)")
                return
            d.close()

    def read(self):
        """Return list of key codes pressed (value 1 or autorepeat 2)."""
        out = []
        if not self.dev:
            return out
        try:
            for ev in self.dev.read():
                if ev.type == e.EV_KEY and ev.value in (1, 2):
                    out.append(ev.code)
        except BlockingIOError:
            pass
        except OSError:
            log("input: device lost; reconnecting")
            try:
                self.dev.close()
            except Exception:
                pass
            self.dev = None
        return out


class UI:
    def __init__(self, lcd):
        self.lcd = lcd
        self.btn = Buttons()
        self.screen = "main"
        self.sel = 0
        self.confirm = None          # "Reboot" / "Shutdown"
        self.overlay = None          # (text, until)
        self.busy = None             # message while a task runs
        self.dirty = True
        self.last_draw = 0.0
        self.status = {}
        self.lock = threading.Lock()

    # --- data ----------------------------------------------------------
    def refresh_status(self):
        b = hw.battery()
        self.status = {
            "wlan0": hw.ipv4("wlan0"),
            "usb0": hw.ipv4("usb0"),
            "batt": b,
            "time": hw.clock_text(),
            "vol": hw.volume_get(),
            "dac": hw.card_present(),
        }

    # --- drawing -------------------------------------------------------
    def header(self, d, title="ZPOD"):
        d.rectangle([0, 0, WIDTH - 1, 29], fill=(20, 20, 60))
        d.text((8, 3), title, font=F_TITLE, fill=ACCENT)
        st = self.status
        right = st.get("time", "")
        if st.get("batt"):
            right = f"{st['batt'][0]:.0f}%  " + right
        w = d.textlength(right, font=F_SMALL)
        d.text((WIDTH - 8 - w, 7), right, font=F_SMALL, fill=FG)

    def footer_ips(self, d, y):
        st = self.status
        d.text((8, y), f"wlan0 {st.get('wlan0') or '--'}", font=F_SMALL, fill=OK if st.get("wlan0") else DIM)
        d.text((8, y + 18), f"usb0  {st.get('usb0') or '--'}", font=F_SMALL, fill=OK if st.get("usb0") else DIM)

    def draw(self):
        img = Image.new("RGB", (WIDTH, HEIGHT), BG)
        d = ImageDraw.Draw(img)
        if self.busy:
            self.header(d)
            d.text((16, 100), self.busy, font=F_BIG, fill=ACCENT)
        elif self.confirm:
            self.header(d)
            d.text((16, 60), f"{self.confirm}?", font=F_BIG, fill=WARN)
            d.text((16, 120), "ENTER = yes", font=F_ITEM, fill=FG)
            d.text((16, 150), "LEFT / X = no", font=F_ITEM, fill=FG)
        elif self.screen == "main":
            self.header(d)
            y = 34
            for i, item in enumerate(MENU):
                if i == self.sel:
                    d.rectangle([4, y, WIDTH - 5, y + 25], fill=SEL_BG)
                d.text((14, y + 2), item, font=F_ITEM, fill=FG)
                y += 27
            self.footer_ips(d, 200)
        elif self.screen == "status":
            self.header(d, "Status")
            st = self.status
            b = st.get("batt")
            lines = [
                ("Time", time.strftime("%Y-%m-%d %H:%M %Z")),
                ("wlan0", st.get("wlan0") or "--"),
                ("usb0", st.get("usb0") or "--"),
                ("Battery", f"{b[0]:.0f}%  {b[1]:.2f} V" if b else "n/a"),
                ("Volume", f"{st['vol']}%" if st.get("vol") is not None else "n/a"),
                ("DAC", "ok" if st.get("dac") else "missing"),
                ("Uptime", hw.uptime()),
            ]
            y = 36
            for k, v in lines:
                d.text((10, y), k, font=F_SMALL, fill=DIM)
                d.text((100, y), v, font=F_ITEM, fill=FG)
                y += 28
        elif self.screen == "volume":
            self.header(d, "Volume")
            v = self.status.get("vol")
            d.text((16, 50), f"{v}%" if v is not None else "n/a", font=F_BIG, fill=ACCENT)
            d.rectangle([16, 110, WIDTH - 16, 140], outline=FG, width=2)
            if v is not None:
                d.rectangle([19, 113, 19 + int((WIDTH - 38) * v / 100), 137], fill=OK)
            d.text((16, 160), "UP/RIGHT +   DOWN/LEFT -", font=F_SMALL, fill=DIM)
            d.text((16, 182), "X / HOME = back", font=F_SMALL, fill=DIM)
        elif self.screen == "wifi":
            self.header(d, "Wi-Fi")
            w = self.wifi or {}
            lines = [("SSID", w.get("ssid") or "--"), ("Signal", f"{w['signal']}%" if w.get("signal") else "--"),
                     ("IP", w.get("ip") or "--"), ("Pwr save", w.get("powersave") or "?"),
                     ("usb0", self.status.get("usb0") or "--")]
            y = 40
            for k, v in lines:
                d.text((10, y), k, font=F_SMALL, fill=DIM)
                d.text((100, y), v, font=F_ITEM, fill=FG)
                y += 30
        if self.overlay and time.monotonic() < self.overlay[1]:
            d.rectangle([40, 90, WIDTH - 40, 150], fill=(30, 30, 30), outline=ACCENT, width=2)
            d.text((60, 105), self.overlay[0], font=F_BIG, fill=ACCENT)
        self.lcd.show(img)
        self.last_draw = time.monotonic()
        self.dirty = False

    # --- actions -------------------------------------------------------
    def run_bg(self, msg, fn):
        def worker():
            self.busy = msg
            self.dirty = True
            try:
                fn()
            finally:
                self.busy = None
                self.dirty = True
        threading.Thread(target=worker, daemon=True).start()

    def select(self):
        item = MENU[self.sel]
        if item == "Status":
            self.screen = "status"
        elif item == "Play test tone":
            def tone():
                ok = hw.test_tone(2, 440)
                self.overlay = ("Tone done" if ok else "Tone FAILED", time.monotonic() + 2.0)
            self.run_bg("Tone 440 Hz", tone)
        elif item == "Volume":
            self.screen = "volume"
        elif item == "Wi-Fi info":
            self.wifi = hw.wifi_info()
            self.screen = "wifi"
        elif item in ("Reboot", "Shutdown"):
            self.confirm = item
        self.dirty = True

    def do_confirmed(self):
        what = self.confirm
        self.confirm = None
        self.busy = "Rebooting..." if what == "Reboot" else "Shutting down"
        self.draw()
        subprocess.run(["systemctl", "reboot" if what == "Reboot" else "poweroff"], check=False)

    def key(self, code):
        K = e
        back = (K.KEY_LEFT, K.KEY_J, K.KEY_ESC, K.KEY_BACKSPACE)
        ok = (K.KEY_ENTER, K.KEY_K, K.KEY_RIGHT)
        if code in (K.KEY_VOLUMEUP, K.KEY_VOLUMEDOWN):
            time.sleep(0.05)                   # let zpod-buttons' amixer land
            self.status["vol"] = hw.volume_get()
            v = self.status["vol"]
            self.overlay = (f"Vol {v}%" if v is not None else "Vol n/a", time.monotonic() + 1.5)
            self.dirty = True
            return
        if self.busy:
            return
        if self.confirm:
            if code == K.KEY_ENTER:
                self.do_confirmed()
            elif code in back or code == K.KEY_HOME:
                self.confirm = None
            self.dirty = True
            return
        if code == K.KEY_HOME:
            self.screen = "main"
        elif self.screen == "main":
            if code == K.KEY_UP:
                self.sel = (self.sel - 1) % len(MENU)
            elif code == K.KEY_DOWN:
                self.sel = (self.sel + 1) % len(MENU)
            elif code in ok:
                self.select()
        elif self.screen == "volume":
            if code in (K.KEY_UP, K.KEY_RIGHT):
                self.status["vol"] = hw.volume_step(+3)
            elif code == K.KEY_DOWN or code == K.KEY_LEFT:
                self.status["vol"] = hw.volume_step(-3)
            elif code in (K.KEY_J, K.KEY_ESC, K.KEY_ENTER):
                self.screen = "main"
        else:
            if code in back or code == K.KEY_ENTER:
                self.screen = "main"
        self.dirty = True

    # --- loop ----------------------------------------------------------
    def loop(self, stop):
        self.wifi = None
        next_status = 0.0
        while not stop.is_set():
            now = time.monotonic()
            if now >= next_status:
                self.refresh_status()
                next_status = now + REFRESH_S
                self.dirty = True
            self.btn.ensure()
            fd = self.btn.fileno()
            timeout = max(0.05, min(next_status - now, 0.5))
            if fd is not None:
                try:
                    r, _, _ = select.select([fd], [], [], timeout)
                except (OSError, ValueError):
                    r = []
                if r:
                    for code in self.btn.read():
                        self.key(code)
            else:
                time.sleep(timeout)
            if self.overlay and time.monotonic() >= self.overlay[1]:
                self.overlay = None
                self.dirty = True
            if self.dirty:
                self.draw()


def main():
    stop = threading.Event()
    signal.signal(signal.SIGTERM, lambda *_: stop.set())
    signal.signal(signal.SIGINT, lambda *_: stop.set())
    lcd = LCD()
    lcd.init()
    log("zpod-ui: LCD initialised (ILI9340-style, MADCTL 0xE8, 320x240)")
    ui = UI(lcd)
    try:
        ui.loop(stop)
    finally:
        lcd.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
