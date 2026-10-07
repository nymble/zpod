# Offline simulation of zpod_buttons (fake GPIO levels + fake uinput). Run: python3 tests/sim_buttons.py
# Simulate zpod_buttons main loop with fake GPIO levels + fake uinput.
import sys, types, time, threading
import os
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'overlay', 'usr', 'lib', 'zpod'))
# fake evdev
ev = types.ModuleType('evdev'); ec = types.ModuleType('evdev.ecodes')
names = ['KEY_UP','KEY_DOWN','KEY_LEFT','KEY_RIGHT','KEY_HOME','KEY_PLAYPAUSE','KEY_I','KEY_U','KEY_K','KEY_J','KEY_VOLUMEUP','KEY_VOLUMEDOWN','KEY_ENTER']
ec.ecodes = {n: i+1 for i, n in enumerate(names)}
ec.NAME = {v: k for k, v in ec.ecodes.items()}
ec.EV_KEY = 1; ec.EV_REP = 20; ec.REP_DELAY = 0; ec.REP_PERIOD = 1
events = []
class UInput:
    def __init__(self, caps, **kw): pass
    def write(self, t, c, v):
        if t == 1: events.append((round(time.monotonic() - T0, 3), ec.NAME[c], v))
    def syn(self): pass
    def close(self): pass
ev.UInput = UInput; ev.ecodes = ec
sys.modules['evdev'] = ev; sys.modules['evdev.ecodes'] = ec
import zpod_buttons as zb
state = {'lev': 0xFFFFFFFF}
class FakeGM:
    def levels(self): return state['lev']
    def pullup_legacy(self, mask): pass
zb.GpioMem = FakeGM
zb.soc_family = lambda: 'bcm2835'
zb.alsa_volume = lambda s: events.append((round(time.monotonic() - T0, 3), 'ALSA', s))
zb.CONF = '/nonexistent'
def press(pins):
    for p in pins: state['lev'] &= ~(1 << p)
def release(pins):
    for p in pins: state['lev'] |= (1 << p)
def script():
    time.sleep(0.1)
    # 1. UP tap 120ms
    press([4]); time.sleep(0.12); release([4]); time.sleep(0.2)
    # 2. quick DOWN tap 35ms (inside combo window)
    press([22]); time.sleep(0.035); release([22]); time.sleep(0.2)
    # 3. centre: 4 pins staggered over 30 ms, hold 150, staggered release
    press([4]); time.sleep(0.01); press([17]); time.sleep(0.01); press([22]); time.sleep(0.01); press([23])
    time.sleep(0.15); release([23]); time.sleep(0.01); release([4, 17]); time.sleep(0.02); release([22]); time.sleep(0.2)
    # 4. bouncy HOME press
    for _ in range(3): press([24]); time.sleep(0.003); release([24]); time.sleep(0.003)
    press([24]); time.sleep(0.1); release([24]); time.sleep(0.2)
    # 5. volume up hold 800 ms (repeat)
    press([5]); time.sleep(0.8); release([5]); time.sleep(0.2)
    # 6. vol down (GPIO20) tap
    press([20]); time.sleep(0.1); release([20]); time.sleep(0.2)
    # 7. RIGHT hold 300ms
    press([23]); time.sleep(0.3); release([23]); time.sleep(0.2)
    import signal, os; os.kill(os.getpid(), signal.SIGINT)
T0 = time.monotonic()
threading.Thread(target=script, daemon=True).start()
zb.main()
for e_ in events: print(e_)
