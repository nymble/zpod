# Licensing

## Dual license (from test8 / v0.13.0 onward)

ZPOD application and library code that Paul / nymble owns is dual-licensed:

- **GPL-3.0-or-later** (`LICENSE-GPL-3.0`), or
- **Commercial** (`LICENSE-COMMERCIAL`) — contact via https://github.com/nymble

Workspace crates declare `license = "GPL-3.0-or-later OR LicenseRef-Commercial"`.

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
  copyright claim; Paul / nymble owns those changes.
- `drivers/aoide-zpod-dac/`: nymble commits on top of GPL-2.0 vendor origin —
  left GPL-2.0, not dual-licensed.

## Trillium

Any Trillium sense / MIDI config protocol support is a clean-room
reimplementation of documented wire formats. No Trillium (GPL-3.0) source is
copied into this tree.
