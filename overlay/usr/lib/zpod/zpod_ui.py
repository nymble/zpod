#!/usr/bin/env python3
"""zpod-ui: status + menu screen on the ZPOD 320x240 SPI LCD.

Input comes from the uinput keyboard published by zpod-buttons (device name
"zpod-buttons"); the UI never touches button GPIOs itself.

  Labels follow the vendor ZPOD user guide (docs/vendor/, docs/BUTTONS.md):
  UP/DOWN        move selection (vendor: repeat / shuffle, kept for the player)
  Circle(K) / centre(ENTER) / RIGHT   OK / select
  X(J) / LEFT    cancel / back         Square(U)      back to menu
  HOME           back to main menu
  PLAY/PAUSE     no-op here (play/pause belongs to the music player, M4)
  VOLUMEUP/DOWN  applied by zpod-buttons (single writer); the overlay shows
                 /run/zpod/volume and stays up while the key is held

Menu: Status, Wi-Fi info, Diagnostics: tone test, Reboot, Shutdown
(Reboot / Shutdown ask for confirmation). M4 replaces this UI with the Rust
launcher (docs/ui/BUTTONS-AND-MENUS.md); this is the fallback.
"""
import os
import select
import signal
import subprocess
import sys
import configparser
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

UI_CONF = "/etc/zpod/ui.conf"
SHUTDOWN_SCREENS = ("message", "off")

MENU = ["Status", "Wi-Fi info", "Diagnostics: tone test", "Reboot", "Shutdown"]


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


def shutdown_screen_mode():
    """[shutdown] screen = message (default) | off, from /etc/zpod/ui.conf."""
    cp = configparser.ConfigParser(inline_comment_prefixes=(";", "#"))
    try:
        cp.read(UI_CONF)
        mode = cp.get("shutdown", "screen", fallback="message").strip().lower()
    except Exception:
        mode = "message"
    return mode if mode in SHUTDOWN_SCREENS else "message"


def system_stop_kind():
    """'poweroff', 'reboot' or None (plain service stop/restart)."""
    try:
        jobs = subprocess.run(["systemctl", "list-jobs", "--no-legend", "--plain"],
                              capture_output=True, text=True, timeout=3).stdout
    except Exception:
        return None
    if "reboot.target" in jobs or "kexec.target" in jobs:
        return "reboot"
    if "poweroff.target" in jobs or "halt.target" in jobs:
        return "poweroff"
    return None


def final_image(kind):
    """High-contrast last frame; the panel keeps it after the Pi halts."""
    img = Image.new("RGB", (WIDTH, HEIGHT), (0, 0, 0))
    d = ImageDraw.Draw(img)
    if kind == "reboot":
        d.text((20, 80), "Rebooting...", font=F_BIG, fill=(255, 255, 255))
        return img
    d.rectangle([6, 6, WIDTH - 7, HEIGHT - 7], outline=(0, 200, 0), width=3)
    d.text((24, 40), "ZPOD off", font=font(44, True), fill=(255, 255, 255))
    d.text((24, 112), "Safe to power off", font=font(26, True), fill=(0, 230, 0))
    d.text((24, 160), "Slide the power switch off.", font=F_SMALL, fill=(200, 200, 200))
    d.text((24, 186), time.strftime("Halted %Y-%m-%d %H:%M"), font=F_SMALL, fill=(150, 150, 150))
    return img


def end_screen(lcd, kind, mode):
    """Leave the panel in a clear state when zpod-ui stops.

    poweroff + mode=message: final 'ZPOD off / Safe to power off' frame,
                             backlight stays on (3V3 stays up after halt).
    poweroff + mode=off:     black, DISPOFF, SLPIN, backlight (GPIO27) low.
    reboot:                  'Rebooting...' frame (zpod-ui re-inits on boot).
    plain stop/restart:      blank + backlight off; Restart= re-inits.
    """
    try:
        if kind == "poweroff" and mode == "message":
            lcd.show(final_image("poweroff"))
            return
        if kind == "reboot":
            lcd.show(final_image("reboot"))
            return
    except Exception:
        pass
    lcd.blank_off()


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
        self.vol_until = 0.0

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
            d.text((16, 120), "OK (Circle / centre) = yes", font=F_ITEM, fill=FG)
            d.text((16, 150), "X / Left = cancel", font=F_ITEM, fill=FG)
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
                ("SD card", hw.sd_summary()),
                ("Build", hw.release_short()),
            ]
            y = 34
            for k, v in lines:
                d.text((10, y), k, font=F_SMALL, fill=DIM)
                d.text((100, y), v, font=F_SMALL if len(v) > 18 else F_ITEM, fill=FG)
                y += 22
        elif self.screen == "wifi":
            self.header(d, "Wi-Fi")
            w = self.wifi or {}
            lines = [("SSID", w.get("ssid") or "--"), ("Signal", f"{w['signal']}%" if w.get("signal") else "--"),
                     ("IP", w.get("ip") or "--"), ("MAC", w.get("mac") or "--"),
                     ("Pwr save", w.get("powersave") or "?"),
                     ("usb0", self.status.get("usb0") or "--")]
            y = 36
            for k, v in lines:
                d.text((10, y), k, font=F_SMALL, fill=DIM)
                d.text((100, y), v, font=F_ITEM, fill=FG)
                y += 28
        vs = hw.volume_state()
        if vs is not None and (vs[1] or time.monotonic() < self.vol_until):
            d.rectangle([50, 88, WIDTH - 50, 152], fill=(30, 30, 30), outline=ACCENT, width=2)
            d.text((66, 94), f"Volume {vs[0]}%", font=F_ITEM, fill=ACCENT)
            d.rectangle([66, 126, WIDTH - 66, 140], outline=FG, width=1)
            d.rectangle([67, 127, 67 + int((WIDTH - 134) * min(vs[0], 80) / 80), 139], fill=OK)
        elif self.overlay and time.monotonic() < self.overlay[1]:
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

    def toggle_tone(self):
        """Play/Pause (or the menu item): start a 440 Hz tone, or stop it."""
        p = getattr(self, "tone_proc", None)
        if p is not None and p.poll() is None:
            p.terminate()
            self.overlay = ("Tone stopped", time.monotonic() + 1.5)
            self.dirty = True
            return
        try:
            self.tone_proc = subprocess.Popen(
                ["timeout", "10", "speaker-test", "-D", "default", "-t", "sine", "-f", "440", "-c", "2"],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            self.overlay = ("Tone 440 Hz", time.monotonic() + 1.5)
        except Exception:
            self.overlay = ("Tone FAILED", time.monotonic() + 2.0)
        self.dirty = True

    def select(self):
        item = MENU[self.sel]
        if item == "Status":
            self.screen = "status"
        elif item == "Diagnostics: tone test":
            self.toggle_tone()
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
            # zpod-buttons already set the volume and /run/zpod/volume; the
            # overlay stays while held and 1.2 s after release
            self.vol_until = time.monotonic() + 1.2
            self.dirty = True
            return
        if self.busy:
            return
        if self.confirm:
            if code in (K.KEY_ENTER, K.KEY_K):
                self.do_confirmed()
            elif code in back or code in (K.KEY_HOME, K.KEY_U):
                self.confirm = None
            self.dirty = True
            return
        if code in (K.KEY_HOME, K.KEY_U):          # Home / Square: back to menu
            self.screen = "main"
        elif code == K.KEY_PLAYPAUSE:                # Play/Pause: stop a running tone only
            p = getattr(self, "tone_proc", None)
            if p is not None and p.poll() is None:
                self.toggle_tone()
        elif self.screen == "main":
            if code == K.KEY_UP:
                self.sel = (self.sel - 1) % len(MENU)
            elif code == K.KEY_DOWN:
                self.sel = (self.sel + 1) % len(MENU)
            elif code in ok:
                self.select()
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
            vs = hw.volume_state()
            if vs is not None and (vs[1] or 0 < self.vol_until - time.monotonic() < 1.3):
                self.dirty = True
                if not vs[1] and time.monotonic() >= self.vol_until:
                    self.vol_until = 0.0
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
        # SIGTERM arrives from systemd on stop, reboot and poweroff. Never
        # leave a stale frame (M2 bug: frozen 'Shutting down' after poweroff):
        # see end_screen() for the final state per case.
        p = getattr(ui, "tone_proc", None)
        if p is not None and p.poll() is None:
            p.terminate()
        kind = system_stop_kind()
        mode = shutdown_screen_mode()
        log(f"zpod-ui: stopping (system={kind or 'running'}, shutdown screen={mode})")
        end_screen(lcd, kind, mode)
        lcd.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
