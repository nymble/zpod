# Notices and attribution

## This project

Copyright (C) 2026 Paul Lambert (d/b/a M-of-N). Licensed under
`GPL-3.0-or-later OR LicenseRef-Commercial`; see [LICENSE](LICENSE),
[LICENSE-COMMERCIAL.md](LICENSE-COMMERCIAL.md) and [LICENSES/](LICENSES/). Contributions:
[CONTRIBUTING.md](CONTRIBUTING.md). The first commit of this repository is dated 2026-10-03.

This replaces the earlier licence notes: the workspace was declared MIT, and a licence
commit on the `test8` branch (tag `licenses-dual-mofn`) used the same expression with the
copyright line "M-of-N 2024-2026". The copyright holder is the person named above.

**Earlier published releases.** Tags up to `v0.12.0-br-ab-test7` and commits through
`525bc84` were published under MIT. Those published copies stay MIT as published;
relicensing is forward-only. Anyone building on this code for new work should use a
revision that carries this notice.

## Names and marks

"ZPOD" is a name used by the hardware vendor (uGeek) and is not claimed by this project. Names
and marks of the copyright holder are not licensed by the GPL or by a commercial licence.

## Known exceptions and third-party material

| Item | Licence | Note |
| --- | --- | --- |
| `drivers/aoide-zpod-dac/` | GPL-2.0-only | Out-of-tree rebuild of the vendor AOIDE ZPOD DAC ASoC module (author string WWW.AOIDE.CC). Not dual-licensed; keep out of the dual-licensed crates. The vendor source repositories declare no licence; the module itself says GPL v2. |
| `docs/vendor/ZPOD-HiFi-Music-Player-User-Guide-2017.pdf` | Vendor copyright (uGeek), `LicenseRef-Vendor-Document` | Third-party document kept unmodified as the hardware reference for the ZPOD (source URL and sha256 in `docs/vendor/README.md`). Copyright stays with uGeek; it is not licensed under the project terms. The Hardware section of `README.md` restates only the hardware facts in our own words. |
| uGeek / Howard Q. repositories (reference only) | Mostly no licence declared | Nothing copied into this repository; see `docs/vendor/HOWARDQIAO.md` where present. |
| Rust dependencies (serde, toml, libc, embedded-graphics, png, jpeg-decoder, miniz_oxide, zlib-rs and others) | MIT, Apache-2.0, Zlib, 0BSD and similar permissive | See `apps/Cargo.lock`. |
| `u8g2-fonts` crate font data | Mixed, per font (U8g2 licences); the crate code is MIT OR Apache-2.0 | UI crates only. Check each font in use before shipping it in a commercially licensed bundle. |
| Buildroot, Linux, BusyBox, Raspberry Pi OS and other packages in built images | Their own licences | Not part of this repository's source. |
| Screenshots | Same as the project | Photographs and captures made for this project. |
