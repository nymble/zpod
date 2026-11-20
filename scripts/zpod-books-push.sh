#!/bin/sh
# Push books to the ZPOD's /data/books (OPDS catalog, docs/LIBRARY.md).
#
#   scripts/zpod-books-push.sh [options] <book file | folder>...
#     -n, --dry-run       show the plan only; write nothing anywhere
#     --into SUBDIR       put them under /data/books/SUBDIR/
#     --verify sha256|size|none   check pushed files (default sha256)
#     -H host  -u user    ZPOD (default 10.12.194.1 over USB, else zpod.local; $ZPOD_USER or you)
#
# Same safety as zpod-tag-and-push.sh: sources are never modified, nothing is
# ever deleted on the ZPOD, a book already there with the same name is left
# alone (same bytes: "same"; different bytes: "CONFLICT", skipped and listed),
# every pushed file is checked by sha256 after the copy, rsync resumes a
# dropped USB link. A folder keeps its layout below it (Author/Title.epub).
# Only book files go: epub pdf mobi azw3 azw kfx cbz cbr fb2 djvu txt.
# SSH keys only (BatchMode, strict host key checking as configured).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
case "${1:-}" in -h|--help|"") sed -n '2,17p' "$0"; [ -n "${1:-}" ] && exit 0 || exit 2 ;; esac
exec python3 "$here/zpod_books_push.py" "$@"
