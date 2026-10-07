# build/

pi-gen is cloned here at build time by `../build.sh --confirm --board pi0|pi0w`.
It is not committed. Generated `pi-gen-config` is also local-only.

`tweaks/` holds M1.1+ package list overlays; `build.sh --confirm` runs
`tweaks/apply.sh` on the clone.

Do not flash SD cards from this tree.
