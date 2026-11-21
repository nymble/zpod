# Licensing

## Dual license (from test8 / v0.13.0 onward)

ZPOD application and library code that M-of-N owns is dual-licensed:

- **GPL-3.0-or-later** (`LICENSE`), or
- **Commercial** (`LICENSE-COMMERCIAL.md`) — contact via https://m-of-n.com

SPDX: `GPL-3.0-or-later OR LicenseRef-Commercial`

Workspace crates declare
`license = "GPL-3.0-or-later OR LicenseRef-Commercial"`.

Copyright (c) 2024-2026 M-of-N (https://m-of-n.com).

## What stays under other licenses

| Path | License | Reason |
|---|---|---|
| `drivers/aoide-zpod-dac/` | GPL-2.0 | Rebuild of the vendor AOIDE / WWW.AOIDE.CC machine driver; see `docs/vendor/HOWARDQIAO.md` |
| Buildroot, Linux, BusyBox, and other build-tree packages | their upstream licenses | not our copyright |
| Already-published GitHub releases tagged under MIT (through `v0.12.0-br-ab-test7`) | MIT as published | relicensing is forward-only |

String constants that name the ALSA card (`sndrpiaoidezpod`) are hardware
identifiers, not derived source.

## Copyright holders checked before relicensing

Per-crate `git log --format='%an <%ae>'` on the `test8` branch:

- Every `apps/crates/*` crate: only **nymble** and/or **Paul** (same person;
  local commits used `Paul <paul@Stellas-Mac.local>` before the GitHub noreply
  address). No outside human contributors.
- Some commits list `Co-Authored-By: Claude …` (AI assistance). That is not a
  copyright claim; M-of-N owns those changes.
- `drivers/aoide-zpod-dac/`: nymble commits on top of GPL-2.0 vendor origin —
  left GPL-2.0, not dual-licensed.

## Trillium / Trillium Base

Any Trillium sense / MIDI config protocol support is a clean-room
reimplementation of documented wire formats. No Trillium (GPL-3.0) source is
copied into this tree.

Crates that Trillium Base may pin (`zpod-voices`, `zpod-midi`,
`zpod-music-core`, and siblings under `apps/crates/`) inherit this dual
license from the workspace. Pin a commit at or after this licensing update;
commits through `525bc84` / tags through `v0.12.0-br-ab-test7` remain MIT as
published.
