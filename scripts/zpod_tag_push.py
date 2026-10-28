#!/usr/bin/env python3
"""Stage, tag, add cover art to, and push albums to the ZPOD.

Called by scripts/zpod-tag-and-push.sh (which supplies a Python with mutagen).
See docs/MUSIC-TAGGING.md. Source folders are only ever read; every change is
made on copies in the work dir, and the copies are what get pushed.
"""
import argparse, datetime, difflib, hashlib, html, json, os, re, shlex, shutil
import subprocess, sys, unicodedata, urllib.parse, urllib.request

AUDIO = {'.flac', '.mp3', '.m4a', '.mp4', '.aac', '.ogg', '.oga', '.opus'}
ART_NAMES = ('cover', 'folder', 'front')
ART_EXT = ('.jpg', '.jpeg', '.png')
NEED = ('artist', 'album', 'title', 'tracknumber')
GENERIC_DIRS = {'music', 'media', 'compilations', 'compilation', 'various', 'various artists',
                'to file', 'tofile', 't0 file', '_in itunes', 'in itunes', 'itunes', 'itunes media',
                'flac', 'mp3', 'downloads', 'albums', 'volumes', 'library', 'unsorted', 'incoming'}
GENRES = {'jazz': 'Jazz', 'classical': 'Classical', 'blues': 'Blues', 'rock': 'Rock', 'folk': 'Folk',
          'country': 'Country', 'soundtrack': 'Soundtrack', 'soundtracks': 'Soundtrack',
          'hip-hop': 'Hip-Hop', 'hip hop': 'Hip-Hop', 'electronic': 'Electronic', 'reggae': 'Reggae',
          'soul': 'Soul', 'funk': 'Funk', 'pop': 'Pop', 'metal': 'Metal', 'punk': 'Punk',
          'bluegrass': 'Bluegrass', 'world': 'World', 'gospel': 'Gospel', 'latin': 'Latin'}
FORMAT_MARK = re.compile(r'\s*[\(\[][^\)\]]*(?:kbps|flac|mp3|alac|aac|\bbit\b|khz|24-?\d+|16-?44|web|vinyl rip|cd rip)[^\)\]]*[\)\]]', re.I)
EDITION = re.compile(r'\s*[\(\[][^\)\]]*(?:remaster|edition|version|deluxe|mono|stereo|expanded|feat\.|legacy|anniversary|rvg|bonus|live|reissue)[^\)\]]*[\)\]]', re.I)
DISC_DIR = re.compile(r'^(?:disc|disk|cd)\s*[-_ ]*\s*(\d{1,2})\b.*$', re.I)
ITUNES = 'https://itunes.apple.com'
LOG = None


def log(msg=''):
    print(msg, flush=True)
    if LOG:
        LOG.write(msg + '\n'); LOG.flush()


def nfc(s):
    return unicodedata.normalize('NFC', s)


def norm(s):
    s = nfc(s).lower().replace('&', 'and')
    s = re.sub(r"[\u2018\u2019'`]", '', s)
    s = re.sub(r'[^a-z0-9]+', ' ', s)
    return re.sub(r'^the ', '', s).strip()


def safe_name(s):
    return nfc(s).replace('/', '-').strip() or '_'


# ---------------------------------------------------------------- parsing

def parse_album_dir(name):
    """'(1958) Miles Davis - Milestones' -> (year, rest). Also 'Album (1958)',
    '1958 - Album', '[1958] Album'. Format markers like [320 kbps] are dropped."""
    s = FORMAT_MARK.sub('', nfc(name)).strip()
    year = None
    for pat, yi, ri in ((r'^[\(\[](\d{4})(?:\s*[-\u2013]\s*\d{2,4})?[\)\]]\s*[-.]?\s*(.+)$', 1, 2),
                        (r'^(\d{4})\s*[-\u2013.]\s+(.+)$', 1, 2),
                        (r'^(.+?)\s*[\(\[](\d{4})[\)\]]$', 2, 1)):
        m = re.match(pat, s)
        if m:
            year, s = m.group(yi), m.group(ri).strip()
            break
    return year, s


def split_artist_album(rest, known_artist=None):
    """'Miles Davis - Milestones' -> ('Miles Davis', 'Milestones')."""
    if ' - ' in rest:
        left, right = rest.split(' - ', 1)
        if known_artist is None or norm(left) == norm(known_artist):
            return left.strip(), right.strip()
    return None, rest


def clean_album(album):
    # "Somethin' Else (Cannonball Adderley album)" -> "Somethin' Else"
    return re.sub(r'\s*\([^)]*\balbum\)$', '', album, flags=re.I).strip()


def artist_from_dir(name):
    n = FORMAT_MARK.sub('', nfc(name))
    n = re.sub(r'\s*[-_]\s*(flac|mp3|alac|aac|lossless|hi-?res)\s*$', '', n, flags=re.I).strip()
    return None if norm(n) in GENERIC_DIRS or not n else n


def parse_track_file(stem):
    """'01 Title', '01 - Title', '01. Title', '1-05 Title' -> (disc, track, title)."""
    s = nfc(stem).strip()
    m = re.match(r'^(\d{1,2})[-.](\d{1,3})(?:\s*[-.]\s*|\s+)(.+)$', s)
    if m and not re.match(r'^\d{1,3}\s', s):
        return int(m.group(1)), int(m.group(2)), m.group(3).strip()
    m = re.match(r'^(\d{1,3})(?:\s*[-._)]\s*|\s+)(.+)$', s)
    if m:
        return None, int(m.group(1)), m.group(2).strip()
    return None, None, s


def genre_from_path(path):
    for part in reversed(path.split(os.sep)):
        g = GENRES.get(part.strip().lower())
        if g:
            return g
    return None


# ---------------------------------------------------------------- tags & art (mutagen)

def mf(path, easy=True):
    import mutagen
    return mutagen.File(path, easy=easy)


def read_tags(path):
    try:
        f = mf(path)
    except Exception as e:  # unreadable file
        return None, str(e)
    if f is None:
        return None, 'unsupported'
    t = {}
    for k in ('artist', 'albumartist', 'album', 'title', 'tracknumber', 'discnumber', 'date', 'genre'):
        try:
            v = f.get(k)
        except Exception:
            v = None
        if v and str(v[0]).strip():
            t[k] = str(v[0]).strip()
    return t, None


def has_embedded_art(path):
    try:
        f = mf(path, easy=False)
    except Exception:
        return False
    if f is None:
        return False
    if hasattr(f, 'pictures') and f.pictures:
        return True
    tags = getattr(f, 'tags', None)
    if tags is None:
        return False
    try:
        keys = list(tags.keys())
    except Exception:
        return False
    return any(k.startswith('APIC') or k == 'covr' or k.lower() == 'metadata_block_picture' for k in keys)


def write_tags(path, values, overwrite):
    f = mf(path)
    changed = []
    for k, v in values.items():
        if v is None:
            continue
        if not overwrite and f.get(k) and str(f.get(k)[0]).strip():
            continue
        try:
            f[k] = [str(v)]
            changed.append(k)
        except Exception as e:
            log(f'      ! cannot set {k} on {os.path.basename(path)}: {e}')
    if changed:
        f.save()
    return changed


def image_mime(data):
    return 'image/png' if data[:8] == b'\x89PNG\r\n\x1a\n' else 'image/jpeg'


def embed_art(path, data):
    from mutagen.flac import Picture
    f = mf(path, easy=False)
    mime = image_mime(data)
    cls = type(f).__name__
    if cls == 'FLAC':
        f.clear_pictures()
        p = Picture(); p.type = 3; p.mime = mime; p.desc = 'Cover'; p.data = data
        f.add_picture(p)
    elif cls in ('MP3', 'EasyMP3'):
        from mutagen.id3 import APIC, ID3
        if f.tags is None:
            f.add_tags()
        f.tags.delall('APIC')
        f.tags.add(APIC(encoding=3, mime=mime, type=3, desc='Cover', data=data))
    elif cls in ('MP4', 'EasyMP4'):
        from mutagen.mp4 import MP4Cover
        fmt = MP4Cover.FORMAT_PNG if mime == 'image/png' else MP4Cover.FORMAT_JPEG
        f['covr'] = [MP4Cover(data, imageformat=fmt)]
    elif cls in ('OggVorbis', 'OggOpus', 'OggFLAC'):
        import base64
        p = Picture(); p.type = 3; p.mime = mime; p.desc = 'Cover'; p.data = data
        f['metadata_block_picture'] = [base64.b64encode(p.write()).decode('ascii')]
    else:
        return False
    f.save()
    return True


# ---------------------------------------------------------------- cover lookup

def http_json(url):
    req = urllib.request.Request(url, headers={'User-Agent': 'zpod-tag-and-push/1'})
    with urllib.request.urlopen(req, timeout=20) as r:
        return json.load(r)


def http_get(url):
    req = urllib.request.Request(url, headers={'User-Agent': 'zpod-tag-and-push/1'})
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.read()


def big_art(url100, px=600):
    return re.sub(r'/\d+x\d+bb\.', f'/{px}x{px}bb.', url100)


def score_candidate(c, artist, album, ntracks):
    cname = EDITION.sub('', c.get('collectionName', ''))
    a, b = norm(cname), norm(album)
    name = 1.0 if a == b else difflib.SequenceMatcher(None, a, b).ratio()
    an, ca = norm(artist or ''), norm(c.get('artistName', ''))
    artist_ok = bool(an) and (an in ca or ca in an)
    tc = c.get('trackCount') or 0
    tracks = 1.0 - min(abs(tc - ntracks), 10) / 10.0 if ntracks else 0.5
    score = 0.7 * name + 0.2 * artist_ok + 0.1 * tracks
    if name == 1.0 and artist_ok:
        conf = 'high'
    elif name >= 0.85 and artist_ok:
        conf = 'medium'
    else:
        conf = 'low'
    return score, conf, name


def itunes_search(artist, album, ntracks):
    """Best iTunes album match, or None. Returns dict with url/title/conf."""
    best = None
    for term in (f'{artist or ""} {album}'.strip(), album):
        q = urllib.parse.urlencode({'term': term, 'entity': 'album', 'limit': 25, 'country': 'US'})
        try:
            res = http_json(f'{ITUNES}/search?{q}').get('results', [])
        except Exception as e:
            log(f'      ! iTunes search failed: {e}')
            continue
        for c in res:
            s, conf, name = score_candidate(c, artist, album, ntracks)
            if name < 0.5:
                continue
            if best is None or s > best['score']:
                best = {'score': s, 'conf': conf, 'id': c.get('collectionId'),
                        'title': c.get('collectionName'), 'artist': c.get('artistName'),
                        'year': (c.get('releaseDate') or '')[:4], 'tracks': c.get('trackCount'),
                        'url': big_art(c.get('artworkUrl100', '')), 'query': term,
                        'page': c.get('collectionViewUrl', '')}
        if best and best['conf'] == 'high':
            break
    return best


def resolve_cover_spec(spec):
    """--cover value: itunes:<id> | http(s) URL | local image path | none."""
    if spec.lower() == 'none':
        return None
    if spec.startswith('itunes:'):
        cid = spec.split(':', 1)[1]
        r = http_json(f'{ITUNES}/lookup?id={urllib.parse.quote(cid)}')['results'][0]
        return {'conf': 'manual', 'id': r.get('collectionId'), 'title': r.get('collectionName'),
                'artist': r.get('artistName'), 'year': (r.get('releaseDate') or '')[:4],
                'tracks': r.get('trackCount'), 'url': big_art(r['artworkUrl100']), 'query': spec,
                'page': r.get('collectionViewUrl', '')}
    if re.match(r'^https?://', spec):
        return {'conf': 'manual', 'url': spec, 'title': '(manual URL)', 'query': spec}
    p = os.path.expanduser(spec)
    if os.path.isfile(p):
        return {'conf': 'manual', 'file': os.path.abspath(p), 'title': '(manual file)', 'query': spec}
    raise SystemExit(f'--cover: cannot use {spec!r} (expect itunes:<id>, a URL, an image file or none)')


# ---------------------------------------------------------------- discovery

def find_albums(root):
    """Every directory that directly holds audio files is an album (or a disc of one)."""
    albums = {}
    for d, dirs, files in os.walk(root):
        dirs[:] = sorted(x for x in dirs if not x.startswith('.'))
        audio = sorted(f for f in files if not f.startswith('.') and os.path.splitext(f)[1].lower() in AUDIO)
        if not audio:
            continue
        disc, album_dir = None, d
        m = DISC_DIR.match(os.path.basename(d))
        if m:  # Album/Disc 1/..., Album/CD2/...
            disc, album_dir = int(m.group(1)), os.path.dirname(d)
        a = albums.setdefault(album_dir, {'src': album_dir, 'files': []})
        for f in audio:
            a['files'].append({'src': os.path.join(d, f), 'disc_dir': disc,
                               'sub': os.path.relpath(os.path.join(d, f), album_dir)})
    return [albums[k] for k in sorted(albums)]


def plan_album(a, root, args, manifest_dir):
    name = os.path.basename(a['src'])
    year, rest = parse_album_dir(name)
    parent = artist_from_dir(os.path.basename(os.path.dirname(a['src'])))
    if os.path.abspath(a['src']) == os.path.abspath(root):
        parent = artist_from_dir(os.path.basename(os.path.dirname(os.path.abspath(root))))
    known = args.artist or parent
    dir_artist, album = split_artist_album(rest, known)
    album = clean_album(album)
    # existing tags (majority vote across the album's files)
    tagged = {}
    for f in a['files']:
        t, err = read_tags(f['src'])
        f['tags'] = t or {}
        f['err'] = err
        for k in ('albumartist', 'artist', 'album', 'date'):
            if f['tags'].get(k):
                tagged.setdefault(k, {}).setdefault(f['tags'][k], 0)
                tagged[k][f['tags'][k]] += 1
    vote = {k: max(v, key=v.get) for k, v in tagged.items()}
    if args.artist:
        artist, src_ = args.artist, '--artist'
    elif vote.get('albumartist') or vote.get('artist'):
        artist, src_ = vote.get('albumartist') or vote.get('artist'), 'existing tags'
    elif dir_artist:
        artist, src_ = dir_artist, 'folder name "Artist - Album"'
    elif parent:
        artist, src_ = parent, 'parent folder'
    else:
        artist, src_ = None, 'TODO: pass --artist'
    if not args.force and vote.get('album'):
        album, alb_src = vote['album'], 'existing tags'
    else:
        alb_src = 'folder name'
    if not year and vote.get('date'):
        year = vote['date'][:4]
    a.update(name=name, artist=artist, artist_src=src_, album=album, album_src=alb_src,
             year=year[:4] if year else None,
             genre=args.genre or genre_from_path(a['src']),
             stage=os.path.join(args.work, safe_name(artist or '_unknown artist'), safe_name(name)))
    return a


# ---------------------------------------------------------------- main steps

def file_sig(p):
    st = os.stat(p)
    return [st.st_size, int(st.st_mtime)]


def stage_album(a, manifest, dry):
    """Copy source files into the work dir unless an identical-source copy exists."""
    copied = 0
    for f in a['files']:
        dst = os.path.join(a['stage'], *[safe_name(x) for x in f['sub'].split(os.sep)])
        f['stage'] = dst
        sig = file_sig(f['src'])
        rec = manifest.get(dst)
        if rec and rec.get('src') == f['src'] and rec.get('sig') == sig and os.path.exists(dst):
            continue
        copied += 1
        if dry:
            continue
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        shutil.copyfile(f['src'], dst + '.part')   # no chflags/ACLs from the NAS
        os.replace(dst + '.part', dst)
        os.utime(dst, (sig[1], sig[1]))
        manifest[dst] = {'src': f['src'], 'sig': sig}
    # sidecar images from the source album folder (never overwrite one in the stage)
    for fn in sorted(os.listdir(a['src'])):
        b, e = os.path.splitext(fn)
        if b.lower() in ART_NAMES and e.lower() in ART_EXT and not dry:
            dst = os.path.join(a['stage'], fn)
            if not os.path.exists(dst):
                os.makedirs(a['stage'], exist_ok=True)
                shutil.copyfile(os.path.join(a['src'], fn), dst)
    return copied


def tag_album(a, args, dry):
    filled = ok = 0
    for f in a['files']:
        disc, track, title = parse_track_file(os.path.splitext(os.path.basename(f['src']))[0])
        if title and a['artist'] and title.lower().startswith(a['artist'].lower() + ' - '):
            title = title[len(a['artist']) + 3:]
        disc = f['disc_dir'] or disc
        if not dry and os.path.exists(f.get('stage', '')):
            f['tags'] = read_tags(f['stage'])[0] or {}
        have = f['tags']
        missing = [k for k in NEED if not have.get(k)]
        if not missing and not args.force:
            ok += 1
            continue
        vals = {'artist': a['artist'], 'albumartist': a['artist'], 'album': a['album'],
                'title': title, 'tracknumber': track, 'discnumber': disc,
                'date': a['year'], 'genre': a['genre']}
        if not a['artist']:
            vals.pop('artist'); vals.pop('albumartist')
        if dry:
            f['would'] = [k for k, v in vals.items() if v is not None and (args.force or not have.get(k))]
            filled += 1
            continue
        changed = write_tags(f['stage'], vals, args.force)
        if changed:
            filled += 1
    return filled, ok


def album_art_state(a):
    for fn in sorted(os.listdir(a['stage'])) if os.path.isdir(a['stage']) else []:
        b, e = os.path.splitext(fn)
        if b.lower() in ART_NAMES and e.lower() in ART_EXT:
            return 'sidecar', fn
    for fn in sorted(os.listdir(a['src'])):
        b, e = os.path.splitext(fn)
        if b.lower() in ART_NAMES and e.lower() in ART_EXT:
            return 'sidecar', fn
    probe = [f.get('stage') if f.get('stage') and os.path.exists(f['stage']) else f['src'] for f in a['files']]
    if probe and has_embedded_art(probe[0]):
        return 'embedded', None
    return 'missing', None


def write_cover_source(a, cand, status):
    p = os.path.join(a['stage'], '.cover-source.txt')
    with open(p, 'a') as fh:
        fh.write(f"# {datetime.datetime.now().isoformat(timespec='seconds')}  status={status}\n")
        fh.write(f"album_folder: {a['name']}\nalbum: {a['album']}\nartist: {a['artist']}\n")
        for k in ('conf', 'title', 'artist', 'year', 'tracks', 'id', 'page', 'url', 'file', 'query', 'score'):
            if cand and cand.get(k) is not None:
                fh.write(f"cover_{k}: {cand[k]}\n")
        fh.write('\n')


def art_album(a, args, overrides, dry, review):
    """Returns a status string; may leave cover.proposed.jpg for review."""
    stage = a['stage']
    proposed = os.path.join(stage, 'cover.proposed.jpg')
    cover = os.path.join(stage, 'cover.jpg')
    key = [k for k in overrides if k in (a['name'], a['album'])]
    state, fn = album_art_state(a)
    if state != 'missing' and not args.force and not key:
        # cover.jpg we fetched earlier but tracks still lack it (e.g. after a re-stage): re-embed
        if state == 'sidecar' and fn == 'cover.jpg' and os.path.exists(os.path.join(stage, '.cover-source.txt')) and not dry and not review:
            data = open(cover, 'rb').read()
            n = sum(1 for f in a['files'] if os.path.exists(f['stage']) and not has_embedded_art(f['stage']) and embed_art(f['stage'], data))
            return f'ok ({fn}{", re-embedded in %d" % n if n else ""})'
        return f'ok ({state}{": " + fn if fn else ""})'
    if key:
        spec = overrides[key[0]]
        cand = resolve_cover_spec(spec)
        if cand is None:
            return 'skipped (--cover none)'
        if os.path.exists(proposed) and not dry:
            os.remove(proposed)
    elif os.path.exists(proposed) and not args.force:
        cand = {'conf': 'kept', 'title': '(existing cover.proposed.jpg)', 'file': proposed}
    else:
        cand = itunes_search(a['artist'], a['album'], len(a['files']))
        if not cand:
            a['cover'] = None
            return 'NOT FOUND (add cover.jpg by hand or --cover)'
    a['cover'] = cand
    if dry:
        return f"would propose [{cand['conf']}] {cand.get('artist','')} - {cand.get('title','')}"
    os.makedirs(stage, exist_ok=True)
    if cand.get('file') != proposed:
        data = open(cand['file'], 'rb').read() if cand.get('file') else http_get(cand['url'])
        open(proposed, 'wb').write(data)
        write_cover_source(a, cand, 'proposed')
    if review or not args.accept_art:
        return f"PROPOSED [{cand['conf']}] {cand.get('artist','')} - {cand.get('title','')}"
    data = open(proposed, 'rb').read()
    os.replace(proposed, cover)
    n = sum(1 for f in a['files'] if embed_art(f['stage'], data))
    write_cover_source(a, cand, f'accepted, embedded in {n} tracks')
    return f"accepted [{cand['conf']}] {cand.get('title','')} -> cover.jpg + embedded in {n}"


def review_page(albums, work):
    rows = []
    for a in albums:
        p = os.path.join(a['stage'], 'cover.proposed.jpg')
        c = os.path.join(a['stage'], 'cover.jpg')
        img = p if os.path.exists(p) else c if os.path.exists(c) else None
        cand = a.get('cover') or {}
        rows.append('<tr><td>%s</td><td><b>%s</b><br>%s<br><small>%s</small></td><td>%s</td><td>%s<br><small>%s</small></td></tr>' % (
            f'<img src="file://{urllib.parse.quote(img)}" width=150>' if img else '(none)',
            html.escape(a['album'] or '?'), html.escape(a['artist'] or 'TODO artist'), html.escape(a['name']),
            html.escape(a.get('art_status', '')),
            html.escape(f"{cand.get('artist','')} - {cand.get('title','')}" if cand else ''),
            html.escape(cand.get('page', '') or cand.get('url', '') or '')))
    out = os.path.join(work, 'review.html')
    with open(out, 'w') as fh:
        fh.write('<meta charset=utf-8><title>ZPOD cover review</title><table border=1 cellpadding=6>'
                 '<tr><th>cover</th><th>album</th><th>status</th><th>matched</th></tr>' + ''.join(rows) + '</table>')
    return out


def ssh_cmd(args):
    return ['ssh', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10', f'{args.user}@{args.host}']


def fix_perms(path):
    for d, dirs, files in os.walk(path):
        os.chmod(d, 0o755)
        for f in files:
            os.chmod(os.path.join(d, f), 0o644)


def rsync_protects_args():
    """rsync >= 3.2.4 passes remote paths verbatim; older rsync and macOS
    openrsync hand them to the remote shell, so spaces must be quoted."""
    v = subprocess.run(['rsync', '--version'], capture_output=True, text=True).stdout
    m = re.search(r'rsync\s+version\s+v?(\d+)\.(\d+)\.(\d+)', v)
    return bool(m) and 'openrsync' not in v and tuple(map(int, m.groups())) >= (3, 2, 4)


def push(albums, args):
    dest = args.dest.rstrip('/')
    verbatim = rsync_protects_args()
    rels = []
    for a in albums:
        fix_perms(a['stage'])
        rels.append(os.path.relpath(a['stage'], args.work))
    artists = sorted({os.path.dirname(r) for r in rels})
    q = ' '.join(shlex.quote(f'{dest}/{x}') for x in artists)
    subprocess.run(ssh_cmd(args) + [f'mkdir -p {q}'], check=True)
    for a, rel in zip(albums, rels):
        log(f'  rsync {rel}')
        cmd = ['rsync', '-rtp', '--partial', '--exclude', '.*', '--exclude', '*.proposed.*',
               '--exclude', '*.part', '-e', 'ssh -o BatchMode=yes', a['stage'],
               f"{args.user}@{args.host}:" + (lambda p: p if verbatim else shlex.quote(p))(f"{dest}/{os.path.dirname(rel)}/")]
        if args.dry_run:
            cmd.insert(1, '-n')
        if args.verbose:
            cmd.insert(1, '-v')
        subprocess.run(cmd, check=True)
    remote = ' '.join(shlex.quote(f'{dest}/{r}') for r in rels)
    subprocess.run(ssh_cmd(args) + [f'find {remote} -type d -exec chmod 755 {{}} + && find {remote} -type f -exec chmod 644 {{}} +'], check=True)
    return rels


def local_files(stage, work):
    out = {}
    for d, dirs, files in os.walk(stage):
        dirs[:] = [x for x in dirs if not x.startswith('.')]
        for f in files:
            if f.startswith('.') or '.proposed.' in f or f.endswith('.part'):
                continue
            p = os.path.join(d, f)
            out[nfc(os.path.relpath(p, work))] = p
    return out


def verify(rels, args):
    files = {}
    for r in rels:
        files.update(local_files(os.path.join(args.work, r), args.work))
    names = sorted(files)
    if args.verify == 'size':
        script = f'cd {shlex.quote(args.dest)} && tr "\\n" "\\0" | xargs -0 stat -c "%s %n"'
    else:
        script = f'cd {shlex.quote(args.dest)} && tr "\\n" "\\0" | xargs -0 sha256sum'
    r = subprocess.run(ssh_cmd(args) + [script], input='\n'.join(names) + '\n', capture_output=True, text=True)
    remote = {}
    for line in r.stdout.splitlines():
        v, _, n = line.partition(' ')
        remote[nfc(n.lstrip(' *'))] = v
    bad = []
    for n in names:
        if args.verify == 'size':
            want = str(os.path.getsize(files[n]))
        else:
            h = hashlib.sha256()
            with open(files[n], 'rb') as fh:
                for chunk in iter(lambda: fh.read(1 << 20), b''):
                    h.update(chunk)
            want = h.hexdigest()
        if remote.get(n) != want:
            bad.append(n)
    return len(names), bad


def main():
    global LOG
    ap = argparse.ArgumentParser(prog='zpod-tag-and-push.sh', description=__doc__)
    ap.add_argument('source', nargs='+', help='album folder, or a folder of album folders')
    ap.add_argument('--artist', help='album artist when folders do not say')
    ap.add_argument('--genre', help='genre for files that have none')
    ap.add_argument('--review', action='store_true', help='plan + propose covers into the work dir, then stop')
    ap.add_argument('-n', '--dry-run', action='store_true', help='write nothing anywhere; show the plan')
    ap.add_argument('--accept-art', action='store_true', help='use the proposed covers (after review)')
    ap.add_argument('--no-art', action='store_true', help='do not look for or add cover art')
    ap.add_argument('--art-only', action='store_true', help='only add cover art; leave tags alone')
    ap.add_argument('--cover', action='append', default=[], metavar='ALBUM=SPEC',
                    help='cover for one album: itunes:<id>, URL, image file, or none')
    ap.add_argument('--force', action='store_true', help='overwrite existing tags / re-pick covers')
    ap.add_argument('--no-push', action='store_true', help='stage and tag only')
    ap.add_argument('--verify', choices=('sha256', 'size', 'none'), default='sha256')
    ap.add_argument('--work', default=os.path.expanduser(os.environ.get('ZPOD_STAGE', '~/zpod-stage/library')))
    ap.add_argument('-H', '--host', default=os.environ.get('ZPOD_HOST', ''))
    ap.add_argument('-u', '--user', default=os.environ.get('ZPOD_USER', os.environ.get('USER', 'pi')))
    ap.add_argument('--dest', default='/srv/zpod/media/music')
    ap.add_argument('-v', '--verbose', action='store_true')
    args = ap.parse_args()
    if args.no_art and (args.art_only or args.accept_art):
        ap.error('--no-art conflicts with --art-only/--accept-art')
    overrides = {}
    for c in args.cover:
        k, sep, v = c.partition('=')
        if not sep:
            ap.error('--cover needs ALBUM=SPEC')
        overrides[nfc(k.strip())] = v.strip()
    args.work = os.path.abspath(args.work)
    dry, review = args.dry_run, args.review
    if not dry:
        os.makedirs(os.path.join(args.work, '.logs'), exist_ok=True)
        LOG = open(os.path.join(args.work, '.logs', datetime.datetime.now().strftime('%Y%m%d-%H%M%S.log')), 'w')
    mpath = os.path.join(args.work, '.zpod-manifest.json')
    manifest = json.load(open(mpath)) if os.path.exists(mpath) else {}

    albums = []
    for src in args.source:
        src = os.path.abspath(os.path.expanduser(src.rstrip('/')))
        if not os.path.isdir(src):
            raise SystemExit(f'not a folder: {src}')
        for a in find_albums(src):
            albums.append(plan_album(a, src, args, args.work))
    if not albums:
        raise SystemExit('no audio files found')
    mode = 'DRY RUN' if dry else 'REVIEW' if review else 'RUN'
    log(f'zpod-tag-and-push [{mode}]  work={args.work}  albums={len(albums)}')
    todo = [a for a in albums if not a['artist']]

    for a in albums:
        log(f"\n== {a['name']}")
        log(f"   artist: {a['artist'] or '??'}  ({a['artist_src']})   album: {a['album']}  ({a['album_src']})"
            f"   year: {a['year'] or '-'}   genre: {a['genre'] or '-'}   files: {len(a['files'])}")
        log(f"   stage:  {a['stage']}")
        bad = [f for f in a['files'] if f['err']]
        for f in bad:
            log(f"   ! unreadable: {f['sub']}: {f['err']}")
        need = [f for f in a['files'] if any(not f['tags'].get(k) for k in NEED)]
        a['tag_status'] = f"{len(need)} of {len(a['files'])} files missing tags" if need else 'all tagged'
        if dry or review:
            if not args.art_only:
                for f in (need if not args.force else a['files'])[:3]:
                    d, t, ti = parse_track_file(os.path.splitext(os.path.basename(f['src']))[0])
                    log(f"     {f['sub']}  ->  disc={f['disc_dir'] or d or '-'} track={t} title={ti!r}")
                if len(need) > 3:
                    log(f'     ... {len(need) - 3} more')
            log(f"   tags:   {a['tag_status']}")
            for f in a['files']:
                f['stage'] = os.path.join(a['stage'], *[safe_name(x) for x in f['sub'].split(os.sep)])
            a['art_status'] = 'not checked (--no-art)' if args.no_art else art_album(a, args, overrides, dry, True)
            log(f"   art:    {a['art_status']}")
            continue
        if not a['artist'] and not args.art_only:
            a['tag_status'] += ' (skipped: no artist, pass --artist)'
            log('   ! no artist: skipping this album; pass --artist')
            a['skip'] = True
            continue
        n = stage_album(a, manifest, False)
        json.dump(manifest, open(mpath, 'w'), indent=0)
        log(f'   staged: {n} copied, {len(a["files"]) - n} already staged')
        if not args.art_only:
            filled, ok = tag_album(a, args, False)
            a['tag_status'] = f'{filled} tagged, {ok} already ok'
        log(f"   tags:   {a['tag_status']}")
        a['art_status'] = 'not checked (--no-art)' if args.no_art else art_album(a, args, overrides, False, False)
        log(f"   art:    {a['art_status']}")

    if not dry:
        log(f"\nreview page: {review_page(albums, args.work)}")
    log('\nSUMMARY')
    for a in albums:
        log(f"  {a['name'][:60]:60} | {a['artist'] or 'TODO artist':22.22} | tags: {a['tag_status']} | art: {a.get('art_status','')}")
    pending = [a for a in albums if str(a.get('art_status', '')).startswith('PROPOSED')]
    if dry or review:
        log('\nNothing pushed (%s).' % ('dry run' if dry else 'review'))
        if pending:
            log('Check the proposed covers (open the review page). Fix any wrong one with\n'
                '  --cover "<album folder>=itunes:<id>|<url>|<file>|none"  or replace cover.proposed.jpg,\n'
                'then run again with --accept-art.')
        return 0
    if pending:
        log(f'\nSTOPPED before push: {len(pending)} album(s) have proposed covers waiting for review.\n'
            'Open the review page, fix any wrong cover (--cover ALBUM=SPEC, or replace cover.proposed.jpg),\n'
            'then re-run with --accept-art (or --no-art to push without covers).')
        return 3
    if todo:
        log(f'\n{len(todo)} album(s) skipped for lack of an artist: ' + ', '.join(a['name'] for a in todo))
    ready = [a for a in albums if not a.get('skip')]
    if args.no_push or not ready:
        log('\nNot pushing (--no-push).' if args.no_push else '\nNothing to push.')
        return 0
    if not args.host:
        args.host = '10.12.194.1' if subprocess.run(['ping', '-c1', '-t1', '10.12.194.1'], capture_output=True).returncode == 0 else 'zpod.local'
    log(f'\nPUSH to {args.user}@{args.host}:{args.dest}')
    subprocess.run(ssh_cmd(args) + [f'mkdir -p {shlex.quote(args.dest)}'], check=True)
    df = subprocess.run(ssh_cmd(args) + [f'df -h {shlex.quote(args.dest)} | tail -1'], capture_output=True, text=True).stdout.strip()
    log(f'  free before: {df}')
    need_b = sum(os.path.getsize(p) for a in ready for p in local_files(a['stage'], args.work).values())
    log(f'  staged size: {need_b / 1e9:.2f} GB (only changed files are sent)')
    rels = push(ready, args)
    if args.verify != 'none':
        n, bad = verify(rels, args)
        log(f'  verify ({args.verify}): {n - len(bad)}/{n} files match' + (f'; MISMATCH: {bad[:10]}' if bad else ''))
        if bad:
            return 4
    out = subprocess.run(ssh_cmd(args) + [f'mpc -q -w update; mpc stats; df -h {shlex.quote(args.dest)} | tail -1'],
                         capture_output=True, text=True).stdout
    log('  ' + out.strip().replace('\n', '\n  '))
    for art in sorted({a['artist'] for a in ready}):
        o = subprocess.run(ssh_cmd(args) + [f'mpc list album albumartist {shlex.quote(art)} | wc -l'], capture_output=True, text=True).stdout.strip()
        log(f'  MPD albums for {art}: {o}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
