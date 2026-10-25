"""Small, dependency-light helpers for ZPOD status: IPs, battery, volume, Wi-Fi."""
import fcntl
import os
import re
import socket
import struct
import subprocess
import time

CARD = "sndrpiaoidezpod"        # ALSA card id of the Aoide ZPOD DAC
MIXER = "Digital"
I2C_BUS = 1
MAX17048_ADDR = 0x36            # fuel gauge (confirmed present on I2C1)


def ipv4(ifname):
    """Return the IPv4 address of ifname or None (SIOCGIFADDR, no subprocess)."""
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        req = struct.pack("256s", ifname[:15].encode())
        res = fcntl.ioctl(s.fileno(), 0x8915, req)
        return socket.inet_ntoa(res[20:24])
    except OSError:
        return None
    finally:
        s.close()


def battery():
    """(percent, volts) from the MAX17048 at 0x36, or None if unreadable."""
    try:
        from smbus2 import SMBus
        with SMBus(I2C_BUS) as bus:
            soc = bus.read_i2c_block_data(MAX17048_ADDR, 0x04, 2)
            vc = bus.read_i2c_block_data(MAX17048_ADDR, 0x02, 2)
        pct = soc[0] + soc[1] / 256.0
        volts = ((vc[0] << 8) | vc[1]) * 78.125e-6
        return max(0.0, min(100.0, pct)), volts
    except Exception:
        return None


def _run(args, timeout=3):
    try:
        return subprocess.run(args, capture_output=True, text=True,
                              timeout=timeout).stdout
    except Exception:
        return ""


def card_present():
    try:
        with open("/proc/asound/cards") as f:
            return ("[" + CARD) in f.read()
    except OSError:
        return False


VOL_STATE = "/run/zpod/volume"


def volume_state():
    """(pct, held) published by zpod-buttons (the only volume writer), or None."""
    try:
        with open(VOL_STATE) as f:
            a = f.read().split()
        return int(a[0]), len(a) > 1 and a[1] == "1"
    except (OSError, ValueError, IndexError):
        return None


def mac_address(ifname):
    try:
        with open(f"/sys/class/net/{ifname}/address") as f:
            return f.read().strip()
    except OSError:
        return None


def volume_get():
    """Digital playback volume in percent (amixer scale) or None."""
    st = volume_state()
    if st is not None:
        return st[0]
    out = _run(["amixer", "-c", CARD, "sget", MIXER])
    m = re.search(r"\[(\d+)%\]", out)
    return int(m.group(1)) if m else None


def test_tone(seconds=2, freq=440):
    """Play a sine tone on the default ALSA device (the DAC). True on success."""
    try:
        return subprocess.run(
            ["timeout", str(seconds), "speaker-test", "-c", "2", "-r", "44100",
             "-t", "sine", "-f", str(freq)],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            timeout=seconds + 5).returncode in (0, 124)  # 124 = stopped by timeout
    except Exception:
        return False


def wifi_info():
    """Dict with ssid, signal (%), ip, powersave for wlan0 (best effort)."""
    info = {"ssid": None, "signal": None, "ip": ipv4("wlan0"), "powersave": None,
            "mac": mac_address("wlan0")}
    out = _run(["nmcli", "-t", "-f", "ACTIVE,SSID,SIGNAL", "device", "wifi", "list",
                "--rescan", "no"])
    for line in out.splitlines():
        parts = line.split(":")
        if len(parts) >= 3 and parts[0] == "yes":
            info["ssid"] = ":".join(parts[1:-1])
            info["signal"] = parts[-1]
            break
    ps = _run(["iw", "dev", "wlan0", "get", "power_save"])
    m = re.search(r"Power save:\s*(\w+)", ps)
    if m:
        info["powersave"] = m.group(1)
    return info


def uptime():
    try:
        with open("/proc/uptime") as f:
            s = int(float(f.read().split()[0]))
    except OSError:
        return "?"
    d, s = divmod(s, 86400)
    h, s = divmod(s, 3600)
    m, _ = divmod(s, 60)
    return (f"{d}d " if d else "") + f"{h}h{m:02d}m"


def clock_text():
    return time.strftime("%H:%M")


# --- SD card use and build identity (M4.1) -----------------------------
RELEASE_FILE = "/etc/zpod-release"


def release(path=RELEASE_FILE):
    """Build identity from /etc/zpod-release (KEY=VALUE), written at image build."""
    r = {}
    try:
        with open(path) as f:
            for line in f:
                line = line.strip()
                if not line or line.startswith("#") or "=" not in line:
                    continue
                k, v = line.split("=", 1)
                r[k.strip()] = v.strip().strip("'\"")
    except OSError:
        pass
    return r


def release_short(path=RELEASE_FILE):
    r = release(path)
    parts = [r.get(k, "") for k in ("ZPOD_VERSION", "ZPOD_GIT", "ZPOD_BUILD_DATE")]
    parts = [p for p in parts if p]
    return "  ".join(parts) if parts else "unknown"


def human(b):
    if b >= 1 << 30:
        g = b / (1 << 30)
        return f"{g:.0f} GB" if g >= 100 else f"{g:.1f} GB"
    return f"{b / (1 << 20):.0f} MB"


def fs_usage(path):
    """(total, used, avail) bytes like df, or None."""
    try:
        st = os.statvfs(path)
    except OSError:
        return None
    total = st.f_blocks * st.f_frsize
    return total, total - st.f_bfree * st.f_frsize, st.f_bavail * st.f_frsize


def sd_card_bytes():
    try:
        with open("/sys/block/mmcblk0/size") as f:
            return int(f.read().strip()) * 512
    except (OSError, ValueError):
        return None


def sd_summary():
    """'14.8 GB card, root 17% used' (percent like df)."""
    out = []
    card = sd_card_bytes()
    if card:
        out.append(f"{human(card)} card")
    u = fs_usage("/")
    if u:
        total, used, avail = u
        pct = -(-used * 100 // (used + avail)) if used + avail else 0
        out.append(f"root {pct}% used")
    return ", ".join(out) or "n/a"

