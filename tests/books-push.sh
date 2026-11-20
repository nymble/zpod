#!/bin/sh
# Offline test of scripts/zpod-books-push.sh (ZPOD_BOOKS_LOCAL: a local dir
# stands in for /data/books; no SSH, no device).
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
P="$here/scripts/zpod-books-push.sh"
fail() { echo "FAIL: $1"; exit 1; }
mkdir -p "$T/src/Ursula K. Le Guin" "$T/dst/Old"
echo epub1 > "$T/src/Ursula K. Le Guin/The Dispossessed.epub"
echo pdf1 > "$T/src/Manual.pdf"
echo notes > "$T/src/notes.docx"
echo keep > "$T/dst/Old/kept.epub"
export ZPOD_BOOKS_LOCAL="$T/dst"
"$P" -n "$T/src" > "$T/out" || fail "dry run exit"
grep -q '2 new' "$T/out" || fail "dry run plan"
[ ! -e "$T/dst/Manual.pdf" ] || fail "dry run wrote"
grep -q 'skip (not a book file).*notes.docx' "$T/out" || fail "non-book skipped"
"$P" "$T/src" > "$T/out" || fail "push exit"
cmp -s "$T/src/Manual.pdf" "$T/dst/Manual.pdf" || fail "pushed pdf"
cmp -s "$T/src/Ursula K. Le Guin/The Dispossessed.epub" "$T/dst/Ursula K. Le Guin/The Dispossessed.epub" || fail "layout kept"
grep -q 'verified (sha256): 2' "$T/out" || fail "verified"
[ -e "$T/dst/Old/kept.epub" ] || fail "never deletes"
"$P" "$T/src" > "$T/out" || fail "second push exit"
grep -q '0 new, 2 already there' "$T/out" || fail "idempotent"
echo changed > "$T/src/Manual.pdf"
r=0; "$P" "$T/src" > "$T/out" || r=$?
[ $r = 3 ] || fail "conflict exit $r"
grep -q 'CONFLICT  Manual.pdf' "$T/out" || fail "conflict listed"
[ "$(cat "$T/dst/Manual.pdf")" = pdf1 ] || fail "conflict must not overwrite"
"$P" --into Inbox "$T/src/Ursula K. Le Guin/The Dispossessed.epub" > "$T/out" || fail "--into exit"
[ -f "$T/dst/Inbox/The Dispossessed.epub" ] || fail "--into"
r=0; "$P" --dest /data/media/music "$T/src" > /dev/null 2>&1 || r=$?
[ $r != 0 ] || fail "only /data/books"
echo "PASS: books push offline (dry run, layout, sha256 verify, no deletes, conflicts kept, --into)"
