#!/usr/bin/env bash
# Copyright (C) 2026 Paul Lambert
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
# M3 offline verifier: same script as M2, with the M3 and M3.1 (no Wi-Fi credentials) checks included.
exec "$(dirname "$0")/verify-m2-image.sh" "$@"
