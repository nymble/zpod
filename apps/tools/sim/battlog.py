#!/usr/bin/env python3
"""Fake battery history for the simulator / screenshots (M6).
Writes <dir>/battery.log and battery-sessions.log ending at 'now':
a charge to 100 %, then a ~4.5 h discharge at ~9 %/h with gauge noise."""
import math, random, sys, time, os
d = sys.argv[1] if len(sys.argv) > 1 else "/tmp/zpod-sim/batt"
os.makedirs(d, exist_ok=True)
random.seed(7)
now = int(time.time())
start = now - int(4.6 * 3600)
lines = []
t = start - 2 * 3600
soc = 62.0
while t < start:                      # charging
    soc = min(100.0, soc + 19.0 / 60)
    lines.append(f"{t} {soc:.2f} {3.95 + soc / 1000:.3f} {19.0 + random.uniform(-2, 2):.2f} 1")
    t += 60
soc = 100.0
while t <= now:                       # on battery
    rate = 9.0 + 2.5 * math.sin((t - start) / 2400.0)
    soc -= rate / 60
    lines.append(f"{t} {soc:.2f} {3.60 + soc * 0.006:.3f} {-(rate + random.uniform(-3, 3)):.2f} 0")
    t += 60
open(os.path.join(d, "battery.log"), "w").write("\n".join(lines) + "\n")
day = 86400
sess = [(now - 9 * day, now - 9 * day + 10 * 3600, 100, 8), (now - 6 * day, now - 6 * day + 7 * 3600, 98, 30),
        (now - 3 * day, now - 3 * day + 11 * 3600, 100, 3)]
open(os.path.join(d, "battery-sessions.log"), "w").write("".join(f"{a} {b} {c:.1f} {e:.1f}\n" for a, b, c, e in sess))
print(f"{d}: {len(lines)} samples, soc now {soc:.1f}")
