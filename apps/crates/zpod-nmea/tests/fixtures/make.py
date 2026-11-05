#!/usr/bin/env python3
"""Regenerate nd105c-cold-start.nmea (valid checksums). Not a hardware capture."""
import os
here = os.path.dirname(os.path.abspath(__file__))
def L(p):
    cs = 0
    for c in p.encode(): cs ^= c
    return "$%s*%02X" % (p, cs)
out = open(os.path.join(here, "nd105c-cold-start.nmea")).read().split("\n#t=")[0].rstrip("\n").splitlines()
out = [l for l in out if l.startswith("#")]
t = 0
def at(ms):
    out.append("#t=%d" % ms)
# 0-2 s: no fix, searching, no satellites tracked
for s in range(3):
    at(s * 1000)
    out += [L("GPGGA,0000%02d.036,,,,,0,00,,,M,0.0,M,,0000" % s), L("GPGSA,A,1,,,,,,,,,,,,,,,"),
            L("GPGSV,1,1,00"), L("GPRMC,0000%02d.036,V,,,,,,,060180,,,N" % s)]
# 3-4 s: satellites in view, still no fix
for s in range(3, 5):
    at(s * 1000)
    out += [L("GPGGA,2231%02d.000,,,,,0,03,,,M,,M,,0000" % s), L("GPGSA,A,1,,,,,,,,,,,,,,,"),
            L("GPGSV,2,1,06,10,63,051,31,12,48,302,28,25,40,190,24,32,20,110,"),
            L("GPGSV,2,2,06,24,15,045,,14,05,330,"),
            L("GPRMC,2231%02d.000,V,,,,,,,051126,,,N" % s)]
# 5-6 s: 2D fix
for s in range(5, 7):
    at(s * 1000)
    out += [L("GPGGA,2231%02d.000,3746.4957,N,12225.1650,W,1,03,4.1,,M,,M,,0000" % s), L("GPGSA,A,2,10,12,25,,,,,,,,,,4.3,4.1,1.0"),
            L("GPGSV,2,1,06,10,63,051,34,12,48,302,31,25,40,190,27,32,20,110,18"),
            L("GPGSV,2,2,06,24,15,045,,14,05,330,"),
            L("GPRMC,2231%02d.000,A,3746.4957,N,12225.1650,W,0.12,0.00,051126,,,A" % s)]
# 7-8 s: 3D fix, 6 used
for s in range(7, 9):
    at(s * 1000)
    out += [L("GPGGA,2231%02d.000,3746.4961,N,12225.1648,W,1,06,1.2,18.4,M,-25.6,M,,0000" % s), L("GPGSA,A,3,10,12,25,32,24,14,,,,,,,2.1,1.2,1.7"),
            L("GPGSV,2,1,07,10,63,051,41,12,48,302,38,25,40,190,35,32,20,110,29"),
            L("GPGSV,2,2,07,24,15,045,26,14,05,330,22,31,02,260,"),
            L("GPRMC,2231%02d.000,A,3746.4961,N,12225.1648,W,0.05,0.00,051126,,,A" % s)]
# 9 s: a corrupted line (bad checksum) amid good data
at(9000)
out += [L("GPGGA,223109.000,3746.4961,N,12225.1648,W,1,06,1.2,18.4,M,-25.6,M,,0000"),
        "$GPGSA,A,3,10,12,25,32,24,14,,,,,,,2.1,1.2,1.7*00", L("GPRMC,223109.000,A,3746.4961,N,12225.1648,W,0.05,0.00,051126,,,A")]
open(os.path.join(here, "nd105c-cold-start.nmea"), "w").write("\n".join(out) + "\n")
