#!/usr/bin/env bash
# M3 offline verifier: same script as M2, with the M3 and M3.1 (no Wi-Fi credentials) checks included.
exec "$(dirname "$0")/verify-m2-image.sh" "$@"
