#!/usr/bin/env python3
"""Generate a small tagged music library + MPD config for simulator tests.

usage: gen-test-library.py <dir>      (needs ffmpeg and Pillow)
Creates <dir>/music, <dir>/playlists, <dir>/mpd.conf (socket <dir>/socket,
port 6601, null output) - run `mpd <dir>/mpd.conf && mpc -h <dir>/socket update`.
Covers: folder art (cover.jpg), embedded art (MP3 APIC), no art, non-ASCII
names, multiple albums per artist, playlists.
"""
import math, os, subprocess, sys
from PIL import Image, ImageDraw, ImageFont

root = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "/tmp/zpod-mpd")
music = os.path.join(root, "music")
pls = os.path.join(root, "playlists")
os.makedirs(music, exist_ok=True)
os.makedirs(pls, exist_ok=True)

ALBUMS = [
    # artist, album, year, format, art, colours, tracks
    ("Nova Echo", "Night Drive", 2024, "flac", "folder", ((120, 40, 160), (250, 60, 110)),
     ["Neon Avenue", "Midnight Freeway", "Afterglow", "City Lights Fade"]),
    ("Nova Echo", "Starlight EP", 2025, "flac", "folder", ((10, 20, 60), (90, 120, 220)),
     ["Starlight", "Orbit"]),
    ("Aurora Lane", "Glass Coast", 2023, "mp3", "embedded", ((20, 110, 130), (240, 220, 150)),
     ["Tidepools", "Lighthouse", "Saltwater"]),
    ("Sigur Fjörd", "Ágætis Dagur", 2022, "flac", "folder", ((30, 60, 40), (190, 210, 170)),
     ["Fyrsti", "Ljós", "Þögn"]),
    ("Lo-Fi Kollektiv", "Rainy Tuesday", 2021, "mp3", None, None,
     ["Coffee Steam", "Window Seat", "Tram 12", "Umbrella", "Late Bus"]),
]

def art(path, title, c1, c2, size=600):
    im = Image.new("RGB", (size, size))
    d = ImageDraw.Draw(im)
    for y in range(size):
        t = y / (size - 1)
        d.line([(0, y), (size, y)], fill=tuple(int(c1[i] * (1 - t) + c2[i] * t) for i in range(3)))
    for k in range(6):
        cx, cy, r = size * 0.42 + k * size * 0.03, size * 0.45, size * 0.2
        d.ellipse([cx - r, cy - r, cx + r, cy + r], outline=(255, 255, 255), width=max(2, size // 150))
    try:
        f = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", size // 12)
    except OSError:
        f = ImageFont.load_default()
    d.text((size * 0.07, size * 0.78), title, fill=(255, 255, 255), font=f)
    im.save(path, quality=88)

playlist_tracks = {"Road Trip": [], "Focus": []}
for artist, album, year, fmt, art_kind, cols, tracks in ALBUMS:
    adir = os.path.join(music, artist, album)
    os.makedirs(adir, exist_ok=True)
    cover = os.path.join(adir, "cover.jpg")
    if art_kind:
        art(cover, album, *cols)
    for n, title in enumerate(tracks, 1):
        dur = 25 + 7 * n
        fn = f"{n:02d} {title}.{fmt}"
        out = os.path.join(adir, fn)
        if os.path.exists(out):
            continue
        freq = 220 * 2 ** ((n * 3 + len(album)) % 12 / 12)
        cmd = ["ffmpeg", "-loglevel", "error", "-y", "-f", "lavfi", "-i",
               f"sine=frequency={freq:.1f}:duration={dur}:sample_rate=44100"]
        if art_kind == "embedded":
            cmd += ["-i", cover, "-map", "0:a", "-map", "1:v", "-c:v", "mjpeg", "-disposition:v", "attached_pic",
                    "-id3v2_version", "3", "-metadata:s:v", "comment=Cover (front)"]
        meta = {"title": title, "artist": artist, "album_artist": artist, "album": album,
                "track": f"{n}/{len(tracks)}", "date": str(year), "genre": "Test"}
        for k, v in meta.items():
            cmd += ["-metadata", f"{k}={v}"]
        cmd += ["-ac", "2"]
        cmd += (["-c:a", "flac"] if fmt == "flac" else ["-c:a", "libmp3lame", "-b:a", "96k"])
        cmd.append(out)
        subprocess.run(cmd, check=True)
        rel = os.path.relpath(out, music)
        if album in ("Night Drive", "Glass Coast"):
            playlist_tracks["Road Trip"].append(rel)
        if album in ("Ágætis Dagur", "Rainy Tuesday") and n <= 3:
            playlist_tracks["Focus"].append(rel)
    if art_kind == "embedded":
        os.remove(cover)  # only the embedded copy remains

for name, items in playlist_tracks.items():
    with open(os.path.join(pls, name + ".m3u"), "w") as f:
        f.write("\n".join(items) + "\n")

with open(os.path.join(root, "mpd.conf"), "w") as f:
    f.write(f'''music_directory "{music}"
playlist_directory "{pls}"
db_file "{root}/db"
state_file "{root}/state"
log_file "{root}/log"
pid_file "{root}/pid"
bind_to_address "{root}/socket"
bind_to_address "127.0.0.1"
port "6601"
audio_output {{
    type "null"
    name "null"
    sync "yes"
    mixer_type "software"
}}
''')
print(root)
