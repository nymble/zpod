#!/usr/bin/env bash
# M4 offline verifier: the M2 script includes the M3, M3.1 and M4 checks
# (it detects the Rust launcher and checks it when present).
exec "$(dirname "$0")/verify-m2-image.sh" "$@"
