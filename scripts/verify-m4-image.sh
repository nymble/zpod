#!/usr/bin/env bash
# Copyright (C) 2026 Paul Lambert
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
# M4 offline verifier: the M2 script includes the M3, M3.1 and M4 checks
# (it detects the Rust launcher and checks it when present).
exec "$(dirname "$0")/verify-m2-image.sh" "$@"
