#!/usr/bin/env python3
# Copyright (C) 2026 Paul Lambert
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
"""zpod-books-push: copy books to the ZPOD's /data/books (see zpod-books-push.sh).

Never deletes, never overwrites a different book, verifies by sha256.
ZPOD_BOOKS_LOCAL=<dir>: push into a local directory instead of over SSH
(offline tests: tests/books-push.sh)."""
import argparse, getpass, hashlib, os, shlex, subprocess, sys, tempfile

EXTS = {'.epub', '.pdf', '.mobi', '.azw3', '.azw', '.kfx', '.cbz', '.cbr', '.fb2', '.djvu', '.txt'}
LOCAL = os.environ.get('ZPOD_BOOKS_LOCAL')


def log(*a):
    print(*a, flush=True)


def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for b in iter(lambda: f.read(1 << 20), b''):
            h.update(b)
    return h.hexdigest()


def collect(srcs, into):
    """[(root, rel, dest_rel)]: root/rel is the file, dest_rel its path under /data/books."""
    out, bad = [], []
    for s in srcs:
        s = os.path.abspath(os.path.expanduser(s))
        if os.path.isfile(s):
            pairs = [(os.path.dirname(s), os.path.basename(s))]
        elif os.path.isdir(s):
            pairs = []
            for d, dirs, files in os.walk(s):
                dirs[:] = sorted(x for x in dirs if not x.startswith('.'))
                for f in sorted(files):
                    if not f.startswith('.'):
                        pairs.append((s, os.path.relpath(os.path.join(d, f), s)))
        else:
            raise SystemExit(f'not found: {s}')
        for root, rel in pairs:
            if os.path.splitext(rel)[1].lower() not in EXTS:
                bad.append(os.path.join(root, rel))
                continue
            if '\n' in rel or rel.startswith('/') or '..' in rel.split(os.sep):
                bad.append(os.path.join(root, rel))
                continue
            out.append((root, rel, os.path.join(into, rel) if into else rel))
    seen = {}
    for r, rel, d in out:
        if d in seen:
            raise SystemExit(f'two sources for /data/books/{d}: {seen[d]} and {os.path.join(r, rel)}')
        seen[d] = os.path.join(r, rel)
    return out, bad


class Target:
    def __init__(self, args):
        self.a = args

    def sh(self, script, stdin=None):
        if LOCAL:
            cmd = ['sh', '-c', script]
        else:
            cmd = ['ssh', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10', f'{self.a.user}@{self.a.host}', script]
        return subprocess.run(cmd, input=stdin, capture_output=True, text=True)

    def dest(self):
        return LOCAL if LOCAL else self.a.dest

    def hashes(self, rels):
        """{rel: sha256 or 'size:N'} for those that exist on the ZPOD."""
        if not rels:
            return {}
        q = shlex.quote(self.dest())
        script = f'cd {q} 2>/dev/null || exit 0; while IFS= read -r f; do [ -f "$f" ] && printf "%s\\t%s\\n" "$(sha256sum < "$f" | cut -d" " -f1)" "$f"; done; exit 0'
        r = self.sh(script, '\n'.join(rels) + '\n')
        if r.returncode != 0:
            raise SystemExit(f'cannot read the ZPOD ({r.stderr.strip()})')
        out = {}
        for line in r.stdout.splitlines():
            h, _, f = line.partition('\t')
            if f:
                out[f] = h
        return out

    def rsync(self, root, rels):
        with tempfile.NamedTemporaryFile('w', delete=False) as lf:
            lf.write('\n'.join(rels) + '\n')
        try:
            if LOCAL:
                dst = LOCAL.rstrip('/') + '/'
                cmd = ['rsync', '-rt', '--partial', '--ignore-existing', '--files-from', lf.name, root + '/', dst]
            else:
                dst = f'{self.a.user}@{self.a.host}:{self.a.dest.rstrip("/")}/'
                cmd = ['rsync', '-rt', '--partial', '--ignore-existing', '--chmod=D755,F644', '--files-from', lf.name,
                       '-e', 'ssh -o BatchMode=yes', root + '/', dst]
            for attempt in range(1, 4):
                if subprocess.run(cmd).returncode == 0:
                    return
                log(f'  ! rsync failed (attempt {attempt}/3), retrying in 5 s')
                subprocess.run(['sleep', '5'])
            raise SystemExit('rsync failed 3 times; run the same command again to resume')
        finally:
            os.unlink(lf.name)


def main():
    ap = argparse.ArgumentParser(prog='zpod-books-push.sh')
    ap.add_argument('src', nargs='+')
    ap.add_argument('-n', '--dry-run', action='store_true')
    ap.add_argument('--into', default='')
    ap.add_argument('--verify', choices=('sha256', 'size', 'none'), default='sha256')
    ap.add_argument('-H', '--host')
    ap.add_argument('-u', '--user', default=os.environ.get('ZPOD_USER') or getpass.getuser())
    ap.add_argument('--dest', default='/data/books', help=argparse.SUPPRESS)
    a = ap.parse_args()
    into = a.into.strip('/')
    if into and ('..' in into.split('/') or '\n' in into):
        raise SystemExit('bad --into')
    if not a.dest.startswith('/data/books'):
        raise SystemExit('books go to /data/books only')
    books, bad = collect(a.src, into)
    for b in bad:
        log(f'  skip (not a book file): {b}')
    if not books:
        log('No book files.')
        return 0
    if not a.host and not LOCAL:
        a.host = '10.12.194.1' if subprocess.run(['ping', '-c1', '-t1', '10.12.194.1'], capture_output=True).returncode == 0 else 'zpod.local'
    t = Target(a)
    where = LOCAL or f'{a.user}@{a.host}:{a.dest}'
    log(f'Books -> {where}  ({len(books)} file(s))')
    local = {d: sha256(os.path.join(r, rel)) for r, rel, d in books}
    there = t.hashes([d for _, _, d in books])
    new, same, conflict = [], [], []
    for r, rel, d in books:
        if d not in there:
            new.append((r, rel, d))
        elif there[d] == local[d]:
            same.append(d)
        else:
            conflict.append(d)
    for d in same:
        log(f'  same      {d}')
    for d in conflict:
        log(f'  CONFLICT  {d}  (a different file with this name is on the ZPOD; left alone)')
    for _, _, d in new:
        log(f'  new       {d}')
    log(f'{len(new)} new, {len(same)} already there, {len(conflict)} conflict(s)')
    if a.dry_run:
        log('Dry run: nothing written.')
        return 3 if conflict else 0
    if not new:
        return 3 if conflict else 0
    r = t.sh(f'mkdir -p {shlex.quote(t.dest())}')
    if r.returncode != 0:
        raise SystemExit(f'cannot create {t.dest()}: {r.stderr.strip()}')
    # one rsync per source root; --into: stage as links so the layout matches
    by_root = {}
    for r_, rel, d in new:
        by_root.setdefault(r_, []).append((rel, d))
    for root, items in by_root.items():
        if into:
            with tempfile.TemporaryDirectory() as st:
                for rel, d in items:
                    p = os.path.join(st, d)
                    os.makedirs(os.path.dirname(p), exist_ok=True)
                    os.link(os.path.join(root, rel), p) if os.stat(root).st_dev == os.stat(st).st_dev else subprocess.run(['cp', '-p', os.path.join(root, rel), p], check=True)
                t.rsync(st, [d for _, d in items])
        else:
            t.rsync(root, [rel for rel, _ in items])
    if a.verify == 'none':
        log('Pushed (not verified: --verify none).')
        return 3 if conflict else 0
    got = t.hashes([d for _, _, d in new])
    badv = [d for _, _, d in new if d not in got or (a.verify == 'sha256' and got[d] != local[d])]
    for d in badv:
        log(f'  VERIFY FAILED  {d}')
    if badv:
        log(f'{len(badv)} file(s) failed the check; run again (nothing is overwritten: remove the bad copy on the ZPOD first)')
        return 4
    log(f'Pushed and verified ({a.verify}): {len(new)} file(s).')
    return 3 if conflict else 0


if __name__ == '__main__':
    sys.exit(main())
