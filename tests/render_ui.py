# Offline render of zpod_ui screens (fake LCD/GPIO/evdev) -> ui_screens.png. Needs Pillow + DejaVu fonts. Run: python3 tests/render_ui.py
import sys, types, time
import os
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'overlay', 'usr', 'lib', 'zpod'))
for m in ('lgpio', 'spidev'):
    sys.modules[m] = types.ModuleType(m)
ev = types.ModuleType('evdev'); ec = types.ModuleType('evdev.ecodes')
for i, n in enumerate(['KEY_UP','KEY_DOWN','KEY_LEFT','KEY_RIGHT','KEY_HOME','KEY_PLAYPAUSE','KEY_I','KEY_U','KEY_K','KEY_J','KEY_VOLUMEUP','KEY_VOLUMEDOWN','KEY_ENTER','KEY_ESC','KEY_BACKSPACE']):
    setattr(ec, n, i + 1)
ec.EV_KEY = 1; ev.ecodes = ec
sys.modules['evdev'] = ev; sys.modules['evdev.ecodes'] = ec
import zpod_lcd
import zpod_hw as hw
hw.battery = lambda: (87.3, 3.98)
hw.volume_get = lambda: 80
hw.card_present = lambda: True
hw.wifi_info = lambda: {'ssid': 'HomeNet', 'signal': '72', 'ip': '192.168.1.81', 'powersave': 'off'}
_ip = {'wlan0': '192.168.1.81', 'usb0': '10.12.194.1'}
hw.ipv4 = lambda i: _ip.get(i)
import zpod_ui
frames = []
class FakeLCD:
    def show(self, img):
        t = time.time(); zpod_lcd.to_rgb565(img); frames.append(img.copy())
ui = zpod_ui.UI(FakeLCD())
ui.wifi = None
ui.refresh_status(); ui.draw()
K = ec
seq = [K.KEY_DOWN, K.KEY_DOWN, ('snap',), K.KEY_UP, K.KEY_UP, K.KEY_ENTER, ('snap',), K.KEY_LEFT, K.KEY_DOWN, K.KEY_DOWN, K.KEY_DOWN, K.KEY_ENTER, ('snap',), K.KEY_J, K.KEY_DOWN, K.KEY_ENTER, ('snap',), K.KEY_LEFT, K.KEY_VOLUMEUP, ('snap',), K.KEY_UP, K.KEY_UP, K.KEY_ENTER, ('snap',)]
snaps = [frames[-1]]
for s in seq:
    if isinstance(s, tuple):
        if ui.dirty: ui.draw()
        snaps.append(frames[-1]); continue
    ui.key(s)
    ui.draw()
from PIL import Image
W = Image.new('RGB', (320 * 4, 240 * 2))
for i, im in enumerate(snaps[:8]):
    W.paste(im, ((i % 4) * 320, (i // 4) * 240))
W.save('ui_screens.png'); print(len(snaps), 'snaps; confirm=', ui.confirm)
