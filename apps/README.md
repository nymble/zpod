# ZPOD M4 userland (Rust)

Cargo workspace for the Pi Zero W (ARMv6, `arm-unknown-linux-musleabihf`).

| Crate | Kind | What |
|---|---|---|
| `zpod-core` | no_std | keys, events, DateTime, platform traits (InputSource, Clock, Storage, Battery, AudioSink), StatusInfo |
| `zpod-drivers` | no_std, embedded-hal 1.0 | MAX17048 fuel gauge, DS3231 RTC |
| `zpod-panel` | no_std, mipidsi 0.10 | ILI9340 model replaying the M2/M3 init (MADCTL 0xE8, no reset pin) |
| `zpod-ui` | no_std + alloc, embedded-graphics | framebuffer with dirty rects, fonts, status bar, lists, dialogs, images |
| `tetris-core` | no_std + alloc | Tetris rules (SRS, kicks, hold, ghost, levels, scoring, T-spins) + renderer |
| `clock` (`clock_core`) | no_std lib + bin | three faces, partial redraws |
| `music` (`music_core`) | no_std lib + bin | MPD browser / player UI |
| `zpod-mpd` | std | small MPD protocol client (incl. albumart/readpicture) |
| `zpod-hal` | std, Linux | spidev + gpio-cdev panel, evdev, sysfs/amixer status, simulator |
| `zpod-apps` | std | manifest, `.zpa` packages, `zpod-app` CLI |
| `launcher` | bin `zpod-launcher` | menu, System screens, runs apps, end screen |
| `bootsplash` | bin `zpod-bootsplash` | early boot splash |
| `drop` | bin `zpod-drop` | HTTP upload server |

```sh
cargo test --workspace            # host unit tests
tools/check-nostd.sh              # bare-metal build of the cores
tools/build-arm.sh                # ARMv6 release + packages into dist/
tools/sim-apps.sh && . tools/sim/env.sh && ZPOD_SIM_SCRIPT=tools/sim/launcher.txt target/debug/zpod-launcher
```

Simulator: `ZPOD_SIM=<dir>` renders to PNG instead of the LCD, and
`ZPOD_SIM_SCRIPT` feeds keys (see `crates/zpod-hal/src/sim.rs`). Screenshots
of every app are in `../screenshots/`.

Docs: `../docs/APPS.md`, `MUSIC-PLAYER.md`, `FILE-TRANSFER.md`, `TEST-M4.md`.
