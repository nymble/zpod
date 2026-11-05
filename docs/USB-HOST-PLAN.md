# ZPOD portable apps, USB host, Wi-Fi client entry, and GPS — engineering spec

| Field | Value |
| --- | --- |
| Document | `/workspace/zpod-plans/portable-apps-usb-gps-2026-11-05.md` |
| Kind | Engineering specification (Markdown file, **not** a Page) |
| Captured | 2026-11-05 (America/Los_Angeles) |
| Status | **Requirements capture + staged plan only.** No implementation claimed. No repo mutations, builds, installs, user-machine actions, or device changes were performed while writing this doc. |
| Revision | 2026-11-05 (America/Los_Angeles): incorporated **Contrail-proposed** portable contracts (§4.4–§4.11) as proposals only — not approved, not independently verified as existing shipping code |
| Audience | ZPOD engineering; shareable handoff context for Contrail (Apple Vision Pro generative app) and other portable generative targets |
| Related product intent | Mouse Music is **one of many** generative music/art apps; shared libraries and fat synthesis must be reusable across ZPOD, desktop, and Contrail Vision Pro |

---

## 0. Non-goals for this capture pass

- Do **not** claim code, packages, drivers, or UI for the items below already exist beyond what is explicitly labeled as current baseline.
- Do **not** invent a product definition of “fat synthesis”; treat it as an explicit **synthesis richness / shared synthesis engine** requirement (see §3.3).
- Do **not** embed real Wi-Fi SSID or password values. This spec uses placeholders `SSID` / `PSK`.
- Do **not** invent pinouts, hub models, or unverified chipset part numbers. Unknowns are labeled **UNKNOWN** or **VERIFY-ON-DEVICE**.
- Do **not** require one UI toolkit across the tiny TFT and visionOS; platforms own presentation.

---

## 1. Problem statement

Paul wants generative engines and apps that:

1. Run as ZPOD apps (Pi Zero W, custom Buildroot, read-only root, `/data` apps) **and**
2. Reuse the same generation / synthesis / algorithm cores on desktop and Contrail (Apple Vision Pro), with **platform-specific UIs**, and
3. Unlock a practical USB-host workflow on the ZPOD: mouse/keyboard → safe Wi-Fi client password entry → Wi-Fi-based load/log/test loop → free the single USB data port for peripherals (GPS first: GlobalSat ND-105C; later SDR, expanded Wi-Fi radios, etc.).

Mouse Music phase 1 is already a generative music app on ZPOD; it must not remain a one-off. Grammar/editor layers, other generative modes, GPS tooling, and Contrail-side reuse all hang off the same portable core split.

---

## 2. Current baseline (facts for planning; not new work)

### 2.1 Hardware / OS

| Item | Current |
| --- | --- |
| Board | Raspberry Pi Zero W, ARMv6, 512 MB RAM |
| OS | Custom Buildroot image; read-only root; apps/data on `/data` (apps typically under `/data/zpod/apps`, appdata under `/data/zpod/appdata`) |
| UI stack | Rust apps + `zpod-ui` (small TFT); Python `zpod-buttons` for onboard controls |
| Audio out | MPD + ALSA to PCM5122 (Aoide) DAC; **output-only** path; exclusive `hw:` device (no dmix) |
| Dev link today | USB CDC-NCM gadget to Mac: Pi `10.12.194.1/28`; Mac gets DHCP from Pi; see existing `USB-GADGET.md` |
| USB topology | **One** SoC USB controller on the data micro-USB port (`USB`, not `PWR`). Gadget **or** host, not both at once |
| Wi-Fi | Available (station client). Credentials must **never** ship in images; first-boot historically via boot-partition `zpod-wifi.txt` (imported then deleted). Runtime keyboard-based client update is **new work** in this plan |
| Dev push | USB SSH + local push scripts; Wi-Fi endpoint must become **configurable** (host/IP/mDNS), not hard-wired only to gadget |

### 2.2 Mouse Music / shared music core (phase 1)

- Phase 1 Music Mouse–style performance layer is installed / implemented on the ZPOD path (splash, play grid, harmony/scales/voicing/synth/voice allocator).
- Shared crate conceptually: `zpod-music-core` (harmony, scales, voicing, synth, voice allocator).
- **Coupling today:** audio lifecycle tied to MPD pause/disableoutput and ALSA/`aplay`-class device ownership on ZPOD. That coupling must be split so the DSP/generation core is platform-neutral.
- Phase 1: **no** generative grammar engine / editor yet (planned later; option C design).
- Input today: onboard buttons via `zpod-buttons`; design docs mention BT mouse/keyboard aspirations and USB host costing the Mac gadget link.

### 2.3 Packaging today

- On-device install format: `.zpa` (gzip tar + `manifest.toml`) for ZPOD ARMv6 musl ELFs — **device package**, not “portable binary across visionOS/desktop.”
- Portable reuse = **shared source crates / libraries + per-platform adapters and UI**, not one fat binary for all targets.

### 2.4 Contrail handoff

- Contrail (Vision Pro generative music app under Nymbus / Contrail) already received a handoff conceptually; this spec is the durable ZPOD-side capture to keep both sides aligned on portable cores, not a claim that Contrail already consumes `zpod-music-core`.

---

## 3. Goals and product principles

### 3.1 Portable engines, platform UIs

| Layer | Portable? | Notes |
| --- | --- | --- |
| Algorithms (scales, harmony, grammar later, generative modes, GPS parse/normalize, etc.) | **Yes** | Pure logic, no ALSA/MPD/visionOS |
| Synthesis / voice allocator / “fat synthesis” capability | **Yes** (core) | See §3.3; audio **sink** is adapted per platform |
| Normalized input / event model | **Yes** | Buttons, HID, pointer, later BT; platform produces events |
| Sample-rate / block / control interfaces + realtime safety rules | **Yes** (contracts) | Implementations may differ; contracts shared |
| UI | **No single forced UI** | TFT/`zpod-ui` vs visionOS/SwiftUI (or whatever Contrail uses) vs desktop |
| Installable ZPOD `.zpa` | ZPOD-only artifact | Built from portable crates for `arm-unknown-linux-musleabihf` |
| Contrail Swift engines (rational patterns, modal gong/bar/bell, Orrery) | **Keep** | Contrail reports existing Swift engines — **do not mandate rewrite to Rust** (§4.10) |

### 3.2 Apps vs tools vs submenus / services

Not every peripheral must be a standalone launcher app.

| Kind | Meaning | Examples |
| --- | --- | --- |
| **App** | Foreground generative / interactive experience | Mouse Music; future generative art/music modes |
| **Tool** | Focused utility, may be a submenu entry or short-lived screen | GPS status/coordinates; Wi-Fi client setup |
| **Service / ownership** | Background or system-owned capability with honest status APIs | USB role manager; Wi-Fi association; GPS reader daemon or library; later SDR |

GPS round 1 is a **tool/screen** with clear device states, not a generative app. SDR / expanded Wi-Fi radios are later tools/services with independent submenu ownership.

### 3.3 “Fat synthesis” (requirement without inventing a brand definition)

**Requirement (as stated):** synthesis used by generative apps must be **rich, shared, and reusable** (“fat synthesis”) across ZPOD, desktop, and Contrail — not a thin per-app beep engine duplicated three times.

**Operational interpretation for implementers (non-normative naming):**

- One shared synthesis + voice-allocation library (or small family of crates) providing substantial voice/timbre/FX capability relative to the platform budget.
- ZPOD remains CPU/RAM constrained (ARMv6, no NEON, exclusive DAC); desktop/Vision Pro may enable heavier patches of the **same** engine API.
- Feature flags / quality tiers allowed; API surface stays shared.
- **Do not** treat “fat synthesis” as a trademarked product name or a claim that a separate product already exists.

### 3.4 Package format vs portable runtime

| Concept | Meaning |
| --- | --- |
| Portable **contract** first | Language-neutral schemas + golden event/render fixtures (§4.10) |
| Portable **source / library** | Per-platform implementations (ZPOD: Rust crates; Contrail: existing Swift engines allowed) that satisfy the same schemas |
| Optional **C ABI** | Versioned opaque handles + POD events if/when cross-language linking is chosen (§4.11) — **proposed**, not required day one |
| Portable **runtime contract** | Generation query, musical events, DSP prepare/render, input begin/update/end, parameters/presets |
| ZPOD **package** | `.zpa` + manifest for on-device install |
| Desktop / visionOS **package** | Native app bundles / Xcode targets / crate or Swift module consumers — **different artifacts** |
| Same binary everywhere | **Out of scope / false** |
| Mandate rewriting Contrail Swift → Rust | **Out of scope / rejected** |

---

## 4. Shared platform contracts (to design/implement; not claimed done)

> **Baseline (§4.1–§4.3):** ZPOD-oriented contracts from the original capture.
> **Contrail proposals (§4.4–§4.11):** concrete contracts **proposed by Contrail** (2026-11-05 steering). Recorded here as **proposed** for alignment — **not** approved implementation, **not** independently verified as already shipping code on either side. Names are descriptive; final schema IDs may change after review.

### 4.1 Normalized input / events (ZPOD baseline)

- Unified event stream: `Button`, `Key`, `PointerRel`/`PointerAbs`, `Axis`, `Hotplug`, `Unsupported`.
- Sources tagged by device id/name so onboard `zpod-buttons` keys do not collide with USB/BT keyboard letters.
- **Non-blocking startup:** apps must be usable with buttons only; USB/BT optional.
- **Hotplug:** connect/disconnect while running updates availability; no hard require-at-launch unless the app opts in.
- **Honest unsupported states:** e.g. “USB host unavailable while gadget active”, “BT not built”, “no HID kernel support”.
- Refine toward Contrail’s begin/update/end model in §4.7; keep ZPOD button semantics as an adapter producing those events.

### 4.2 Audio sink + realtime safety (ZPOD baseline)

- Split **generator/synth** from **sink adapter**:
  - ZPOD adapter: exclusive ALSA PCM5122 path + MPD handoff (pause/disableoutput → open → on exit enableoutput/restore).
  - Desktop adapter: CoreAudio / ALSA / whatever the desktop target uses.
  - Contrail adapter: visionOS / Apple spatial path (see §4.6).
- Shared contracts: sample rate, block size, control-rate vs audio-rate parameter updates, no alloc/locks/syscalls on the audio thread except the sink write.
- Sample-rate policy: negotiate per platform (§4.6); ZPOD phase 1 often **44.1 kHz**, but **do not** freeze the portable contract to Pi-only caps.
- **Latency vocabulary:** keep **buffering latency** (block/period/buffer configuration) distinct from **measured end-to-end latency** (gesture/event → audible/visible result). Never report one as the other.

### 4.3 Control / UI boundary

- Cores expose state + commands + parameter descriptors + visualization snapshots (§4.8); UIs bind.
- Tiny TFT: coarse controls, few screens, `zpod-ui`.
- Vision Pro: spatial / rich UI — **not** a port of TFT layouts.
- **Core must not** contain UI toolkits, MPD control, or Wi-Fi/network setup.

### 4.4 Generation query contract **[Contrail-proposed]**

Deterministic span query (conceptual signature):

```text
query(start, end, seed, state) -> events[]
```

| Rule | Intent |
| --- | --- |
| Half-open spans `[start, end)` | Exact coverage without double-emitting boundary events |
| Exact **rational musical time** | Musical time is rational (bars/beats/subdivisions), not floating sample time |
| Stable **event IDs** | Same query inputs → same IDs; UI/transport can refer to notes across frames |
| `seed` + `state` | Reproducible generation; state carries continuum across adjacent queries |
| Transport / tempo maps **separate** | Map musical time → samples outside (or beside) the generator; generators stay in musical time |

Generators consume **normalized input events** (§4.7), never raw HID.

### 4.5 Musical event model **[Contrail-proposed]**

Events are richer than MIDI note-on only:

| Field / kind | Notes |
| --- | --- |
| Note / voice / instrument IDs | Stable IDs within a session/preset as defined by schema |
| Pitch | Continuous (**cents** and/or **Hz**), **not MIDI-only** (MIDI may be an adapter view) |
| Energy | Amplitude / intensity dimension distinct from MIDI velocity-only thinking |
| Gate duration | How long the note is held in musical or scheduled terms |
| Expression | Additional continuous/expressive dimensions as schema evolves |
| Note-off | Explicit releases |
| All-notes-off | Panic / scene clear |
| Scheduled parameters | Automations / timed param changes aligned to the same time base |

### 4.6 DSP prepare / render contract **[Contrail-proposed]**

Conceptual lifecycle:

```text
prepare(sampleRate, maxBlock, channels) -> Result
render(outBuffers, sampleOffsetEvents, ...) -> Result   # caller-owned buffers
reset() -> ()
```

| Rule | Intent |
| --- | --- |
| Caller-owned buffers | Explicit buffer ownership; engine does not allocate output in `render` |
| Sample-offset events | Musical/transport events already mapped into this block’s sample offsets |
| No allocation / locks / I/O in `render` | Hard realtime safety |
| Negotiate sample rates | Per-platform capability, not “Pi Zero only” |
| Channels | Mono/stereo baseline; **Apple spatial** is a platform adapter on top (Contrail), not a Pi requirement |
| Capabilities | Advertise what this build can do; do **not** define the portable max as Pi Zero W caps |
| `reset` | Clear voices/internal state without implying UI/network side effects |

### 4.7 Normalized input (begin / update / end) **[Contrail-proposed]**

| Rule | Intent |
| --- | --- |
| Phases | `begin` / `update` / `end` per contact or gesture |
| Timestamps | Monotonic clocks |
| IDs | `source` ID + `contact` ID |
| Axes / pressure | Documented units and frame (e.g. normalized UI, physical, musical) |
| Adapters | Mouse, keyboard, pads, MIDI, hands (Vision), ZPOD buttons — **outside** generators |
| Forbidden in generators | Raw HID reports, platform window events, Wi-Fi/MPD |

ZPOD USB/BT HID work (§6) feeds these adapters; Mouse Music startup chooser remains valid.

### 4.8 Parameters, presets, visualization **[Contrail-proposed]**

| Piece | Intent |
| --- | --- |
| Parameter descriptors | Stable ID, type, range, units, curve, default |
| Presets / state | **Versioned** so hosts can migrate or reject |
| Visualization snapshots | Read-only views for UI meters/grids without touching RT render |
| Core exclusions | No UI widgets, no MPD, no Wi-Fi/client networking inside core libs |

### 4.9 App-specific synthesis needs (same contracts, different engines)

| App / surface | Needs (requirements, not claims of shared code today) |
| --- | --- |
| **Contrail Orrery** | Modal synthesis, timed strikes, polymeter, spatial positions |
| **Mouse Music** | Harmony, scales, voicing, performance grid → shared fat synthesis / voice allocator on ZPOD path |
| Future generative apps | Broad algorithms + their own UIs on top of the same contracts |

Engines may differ; schemas and fixtures align them.

### 4.10 Interop strategy: schemas and fixtures first **[Contrail-proposed]**

1. **Language-neutral schemas** for generation queries, musical events, DSP prepare/render messages, input events, parameter descriptors, presets.
2. **Golden event / render fixtures** (inputs → expected event lists / reference buffers or hashes) shared across ZPOD and Contrail CI.
3. **Do not mandate rewriting** Contrail’s existing Swift rational-pattern / modal gong–bar–bell engine into Rust. Keep Swift engines; implement or adapt toward the shared schemas.
4. ZPOD continues Rust for on-device apps; optional later bridging via §4.11.

### 4.11 Optional versioned C ABI **[Contrail-proposed; optional]**

If cross-language in-process linking is chosen later:

- Versioned C ABI
- Opaque handles for engines/contexts
- POD event structs (or explicit packed POD blobs) matching schemas
- Explicit buffer ownership and explicit error codes
- **Not** required before schemas + fixtures exist

### 4.12 Buffering vs measured end-to-end latency

| Term | Meaning |
| --- | --- |
| Buffering latency | Configured audio period/buffer/block delay (calculable from `sampleRate` × frames) |
| Measured E2E latency | Empirically measured path: input adapter → generation/DSP → DAC/spatial output → perception/test harness |

Document both separately in benches and acceptance notes. Do not substitute buffer math for E2E measurements (or vice versa).

---

## 5. USB host on Pi Zero W — constraints and validation plan

### 5.1 Hard constraint (verified from public Pi docs/forums + current ZPOD gadget docs)

- Pi Zero W exposes **one** USB controller on the data port.
- Current ZPOD image forces peripheral gadget: `dtoverlay=dwc2,dr_mode=peripheral` + CDC-NCM configfs gadget.
- **Cannot** host HID (or GPS) on that port **while** remaining a gadget to the Mac.
- Official Raspberry Pi OTG guidance: when the Zero’s only data port is used for OTG/gadget toward a PC, you cannot also plug keyboard/mouse there; use Wi-Fi/SSH instead for host-side development once networked.
- Host vs peripheral switching is a **mode change** (overlay/`dr_mode`, module unload/reload, and/or reboot). Runtime seamless dual-role is **not** assumed. Treat reliable switch procedure as **VERIFY-ON-DEVICE**.

### 5.2 Power / OTG / hub checklist (no guessed pinouts)

Before declaring HID or GPS supported on a given cable/hub:

1. **PWR IN** vs **USB** ports: keep 5 V supply on `PWR` when backpowering from a host/hub is uncertain; avoid dual 5 V fights.
2. Confirm adapter/hub asserts OTG **ID** correctly for host mode (micro-B OTG adapter or hub designed for host).
3. Measure or budget current: ND-105C datasheet-class draw ~**40 mA** @ 5 V (public manual); keyboard/mouse usually low; bus-powered hub + Pi + several devices may need a **powered hub** or stronger PSU — **VERIFY**, do not assume.
4. Prefer connecting hub/peripherals before concluding “unsupported”; charge-only cables fail silently.
5. Log `dmesg` / `lsusb` evidence in acceptance notes; do not claim compatibility for untested hubs.

### 5.3 Kernel / userspace validation (Buildroot)

**Acceptance probes (host mode session):**

| Check | Pass criteria |
| --- | --- |
| Controller mode | `dr_mode=host` (or equivalent verified host) active; gadget functions **down** |
| USB tree | `lsusb` shows hub/HID/GPS as applicable |
| HID | `CONFIG_HID`, `CONFIG_USB_HID`, input event nodes under `/dev/input/event*` |
| Keyboard | `evtest` or equivalent sees keys; mapped into normalized events |
| Mouse | relative motion + buttons |
| Serial GPS | `usbserial` + `pl2303` (or successor) present; `/dev/ttyUSB*` or stable symlink |
| Hotplug | unplug/replug produces add/remove without reboot |
| Fallback | With no HID, Mouse Music (and tools) still run on buttons |

**UNKNOWN until image audit:** whether current Buildroot defconfig already enables `pl2303`, `usbhid`, hub drivers, and needed udev rules. Mark **VERIFY-ON-IMAGE**.

### 5.4 USB role manager (proposed service ownership)

A small system-owned component (name TBD) should:

- Know current role: `gadget-mac` | `host` | `transition` | `failed`.
- Refuse host-only tools with crisp UI when role is gadget.
- Sequence: ensure Wi-Fi (or other) fallback **before** dropping gadget; verify association + IP; then switch; provide restore-to-gadget path for Mac tether.
- Never log Wi-Fi PSK or other secrets.

---

## 6. Mouse Music startup / input modes

### 6.1 Startup prompt (phase of this plan)

On launch (or first-run settings):

1. **Use ZPOD buttons only** (always available).
2. **Connect USB mouse and/or keyboard** (requires USB **host** role; show prerequisite if gadget active).
3. **Later: Bluetooth** mouse/keyboard (separate milestone; mark unsupported until stack exists).

Behavior:

- Non-blocking: default to buttons if user skips or device missing.
- If USB chosen but no device within timeout: usable fallback + persistent “waiting for HID” affordance, not a hard error exit.
- Hotplug after start attaches control without restart when possible.

### 6.2 Mapping (normative intent; refine in app design)

- Buttons: existing D-pad / OK / face buttons semantics remain authoritative on ZPOD.
- USB mouse: pointer drives Music Mouse grid; buttons for latch/menu/etc. (align with DESIGN.md intent).
- USB keyboard: arrows/Enter/Esc/… plus text entry when a field needs it.
- Do not treat HID as required for phase-1 generative play.

---

## 7. Wi-Fi client password entry and Wi-Fi dev loop

### 7.1 Why this stage exists

USB gadget is the current reliable Mac link. Hosting HID/GPS requires leaving gadget mode. Therefore:

1. Get keyboard (or other safe entry) while still able to recover.
2. Enter Wi-Fi client credentials **on device**.
3. Verify association + IP.
4. Switch USB to host for peripherals / keep Wi-Fi as primary load/log/test path.
5. Make push/SSH scripts take a **configurable endpoint** (mDNS name, Wi-Fi IP, or gadget IP).

### 7.2 Security / UX requirements

- On-device masked password entry via USB keyboard (buttons-only entry is optional later; do not block on it).
- **No secrets in logs**, crash dumps, or screenshots.
- Do not write PSK to world-readable paths; follow least-privilege NetworkManager (or equivalent) secret storage already used by the image.
- Prefer ephemeral UI buffer → NM connection profile; clear UI buffer after apply.
- Preserve **fallback**: if association fails, remain reachable (gadget still up if not yet switched; or known recovery). **Never** switch USB role until Wi-Fi (or alternate) connectivity is verified **or** user explicitly accepts offline host mode.

### 7.3 Verification steps (acceptance)

1. Scan or enter SSID (placeholder in docs only).
2. Enter PSK via keyboard; apply.
3. Confirm `wlan0` associated, has IPv4 (and optionally IPv6), ping or SSH from Mac over Wi-Fi.
4. Only then offer “Switch USB to host”.
5. After switch: SSH/deploy over Wi-Fi works; gadget address `10.12.194.1` may be unavailable — scripts must not assume it.

### 7.4 Configurable Wi-Fi endpoint for tooling

- Env/flag/config: e.g. `ZPOD_HOST=zpod.local` or explicit IP; document gadget default `10.12.194.1` as one profile among several.
- No hardcoded single path in new tooling.

---

## 8. GPS — GlobalSat ND-105C (first peripheral tool)

### 8.1 Round-1 product scope

Basic interface usage and a **crisp info / coordinates** screen with honest prompts:

- Device detection
- Fix freshness / time
- Coordinates (high readability on TFT)
- Reception: satellites in view/used, signal bars **if provided by sentences**
- States: **no-fix** vs **no-device** vs **unsupported** vs **stale/error**

Out of scope for round 1: mapping UI, dead-reckoning, PPS/chrony hard-realtime time sync, SiRF/MTK advanced binary tuning beyond what is needed for NMEA fix display.

### 8.2 Public research summary (labeled)

Sources consulted 2026-11-05 (public web):

| Claim | Support | Confidence |
| --- | --- | --- |
| Model ND-105C USB GNSS mouse-style receiver | GlobalSat materials / reseller pages | High |
| Speaks **NMEA 0183** by default | GlobalSat ND-105C user manual (RU mirror PDF) | High |
| Default serial **4800 8N1** | Same manual; field reports | High |
| Sentences include **GGA, GSA, GSV, RMC** | Manual | High |
| USB-UART bridge **Prolific PL2303** | Manual (Windows/Android PL2303 driver story); Linux field reports; udev examples using VID/PID | High |
| Linux drivers | `usbserial` + `pl2303` → `/dev/ttyUSB*` | High (standard mainline) |
| USB IDs commonly **067b:2303** | Linux `pl2303.h`; udev examples for this class of dongle | High for PL2303 **bridge**; **not** a unique GlobalSat-only ID |
| GNSS chipset “MTK”, 66 channels, ~40 mA @ 5 V | Manual | High for family claims |
| Exact MTK part (e.g. “MTK-3337”) | Third-party driver aggregator pages | **UNKNOWN / VERIFY** — do not treat as confirmed |
| “SiRF binary” also mentioned | Manual lists binary SiRF-style management; Jim Carter notes MediaTek type unknown but SiRF-ish control sentences in the wild | **Mixed** — prefer NMEA for round 1 |
| Cold/warm/hot start times ~35 s / 35 s / 1 s typical | Manual | Medium (conditions vary) |
| Works on Raspberry Pi with gpsd / raw NMEA | Pi forum threads for ND-105C | Medium (host mode + drivers required) |

**Primary links:**

- James F. Carter ND-105C + Linux notes (PL2303, NMEA, udev `067b:2303`): https://www.jfcarter.net/~jimc/documents/gps-clock-1507.html
- GlobalSat ND-105C user manual (RU PDF mirror): https://www.globalsat.ru/Sites/global/Uploads/ND-105C_User_Manual_Rus.0FDB23A531AC4CCC9B7D6A71B5B50EC6.pdf
- Linux PL2303 IDs: https://github.com/torvalds/linux/blob/master/drivers/usb/serial/pl2303.h
- gpsd troubleshooting (USB serial class): https://gpsd.io/troubleshooting.html
- Raspberry Pi OTG whitepaper (Zero: gadget uses the only data port → no concurrent keyboard/mouse): https://pip-assets.raspberrypi.com/categories/685-app-notes-guides-whitepapers/documents/RP-009276-WP-1-Using-OTG-mode-on-Raspberry-Pi-SBCs
- Pi forums: host and gadget not simultaneous on Zero: https://forums.raspberrypi.com/viewtopic.php?t=347847
- Existing ZPOD gadget doc (CDC-NCM, `10.12.194.1`, `dr_mode=peripheral`): in-tree `docs/USB-GADGET.md` (repo reference; not modified in this pass)

### 8.3 Detection and protocol plan

1. Require USB **host** role.
2. Match USB device: prefer PL2303 interface; optionally bind stable symlink (e.g. `/dev/gps0`) via udev when VID/PID match — remember **067b:2303 is chip-class**, not GlobalSat-unique if multiple PL2303 devices exist.
3. Open serial **4800 8N1 raw**; read NMEA; validate checksums.
4. Parse at least RMC/GGA for fix + lat/lon/alt/time; GSA for fix type / sats used; GSV for elev/az/SNR bars.
5. Optional later: `gpsd` as a service owner vs in-app reader — decide in implementation; round 1 can be a dedicated reader library for fewer moving parts on Buildroot (**TBD**).

### 8.4 UI states (normative)

| State | User-visible meaning |
| --- | --- |
| `unsupported` | Image lacks drivers, or USB role is gadget, or serial subsystem missing |
| `no-device` | Host OK; no matching USB device |
| `device-present-no-data` | TTY open but no valid NMEA (cable/power/firmware) |
| `no-fix` | Valid NMEA, no 2D/3D fix yet (searching) |
| `fix` | Fresh fix within freshness window; show coordinates + time + sat/SNR |
| `stale` | Had fix; no update within freshness threshold |
| `error` | I/O errors, checksum failures storm, unexpected disconnect |

Freshness threshold: propose **2–5 s** without new fix sentence → `stale` (**tune on device**).

Coordinates: crisp large lat/lon (decimal degrees and/or DMM), altitude if present, UTC time from sentences, sats used/in view, simple SNR bar row from GSV when available.

### 8.5 Ownership

- Prefer **GPS tool/submenu** + optional small reader service, not a generative app.
- Other apps may later subscribe to normalized location events; round 1 does not require that bus.

---

## 9. Later USB tools (placeholders only)

Independent submenu/service ownership after GPS round 1:

| Tool | Intent | Notes |
| --- | --- | --- |
| SDR | USB SDR receiver apps/tools | Host mode; power/hub critical; **no** driver claims yet |
| Expanded Wi-Fi | Extra USB radios / bands / features | Conflicts possible with onboard WLAN; isolate as experiment |
| HID | Mouse/keyboard (this plan’s early stage) | Prerequisite for typed Wi-Fi PSK |
| GPS | ND-105C | First non-HID peripheral milestone |

---

## 10. Staged implementation plan (actionable order)

> Each stage ends with acceptance tests. Do not start the next stage’s device-mode assumptions until the previous stage’s recovery path works.

### Stage A — Spec & contracts (this document)

- [x] Capture requirements as durable Markdown (this file).
- [x] Record Contrail-proposed contracts (§4.4–§4.11) as **proposed**, not approved/verified shipping code.
- [ ] Review with Paul / Contrail; accept, revise, or reject proposals explicitly.
- [ ] Draft language-neutral schemas + golden event/render fixture format (before ABI or forced shared language).
- [ ] Inventory Buildroot kernel features for HID + PL2303 (**VERIFY-ON-IMAGE**).

### Stage B — Portable core split + schema alignment (software; no USB role change required)

1. Publish draft schemas/fixtures for `query`, musical events, `prepare`/`render`, input begin/update/end, parameters/presets (**proposed contracts → review**).
2. Extract platform-neutral ZPOD crates from Mouse Music / `zpod-music-core`: harmony, scales, voicing, synth/voice allocator, (later) grammar — adapters keep MPD/ALSA.
3. Map Contrail Swift engines (rational patterns, modal gong/bar/bell, Orrery needs: modal synthesis, timed strikes, polymeter, spatial positions) onto the **same schemas** without mandating a Rust rewrite.
4. Fat-synthesis / DSP quality tiers per **platform capabilities** (not Pi-only caps in the contract); Apple spatial remains a Contrail adapter.
5. Optional spike: versioned C ABI opaque handles only **after** fixtures exist.
6. Acceptance: fixtures pass on at least one ZPOD-side and one Contrail-side implementation path; Mouse Music still runs; core has no UI/MPD/Wi-Fi; latency notes separate buffering vs measured E2E.

### Stage C — USB role management design + host bring-up

1. Document exact switch procedure for this image (reboot vs module reload) — **VERIFY-ON-DEVICE**.
2. Implement role status API + UI honesty in Settings/Tools.
3. Host-mode image or boot alternative: `dr_mode=host`, HID + hub + pl2303 enabled.
4. Acceptance: with known-good OTG adapter, keyboard produces `/dev/input/event*`; gadget absent; Wi-Fi or serial console still available for recovery.

### Stage D — HID in Mouse Music startup

1. Startup chooser: buttons / USB HID / (later BT).
2. Normalized HID → Music Mouse controls; hotplug; fallback.
3. Acceptance: playable without HID; with HID, pointer/keys work; unplug returns to buttons without crash.

### Stage E — On-device Wi-Fi client update via keyboard

1. Wi-Fi tool: SSID + PSK entry; NM apply; no secret logging.
2. Verify association + IP before offering USB role switch.
3. Update deploy scripts for configurable `ZPOD_HOST`.
4. Acceptance: Mac SSH over Wi-Fi; failed PSK leaves prior connectivity; logs scrubbed.

### Stage F — Wi-Fi-first load / log / test loop

1. Day-to-day app push/test over Wi-Fi.
2. Keep gadget profile as optional tether when host peripherals not needed.
3. Acceptance: full install/run/log cycle without USB gadget.

### Stage G — GPS tool (ND-105C)

1. Host-mode detection + NMEA reader + TFT status UI + states in §8.4.
2. Optional udev symlink; freshness timers; SNR bars from GSV.
3. Acceptance: all states demonstrable (unplug, indoor no-fix, outdoor fix); no panic on unplug; unsupported when in gadget mode.

### Stage H — Additional generative apps / modes

1. New apps reuse portable cores + fat synthesis; ship as separate `.zpa` on ZPOD.
2. Grammar/editor (Mouse Music phase 2+) as library + ZPOD UI + Contrail UI separately.
3. Acceptance: second app links shared crates without copying synth code.

### Stage I — SDR / expanded Wi-Fi experiments

1. Only after stable host role + power budget known.
2. Separate tool ownership; do not block music cores.

---

## 11. Cross-cutting acceptance tests

| ID | Test | Pass |
| --- | --- | --- |
| P1 | Portable crate builds for ZPOD target and at least one desktop target | Both build; synth smoke test |
| P2 | ZPOD `.zpa` ≠ visionOS artifact | Documented separate ship paths |
| C1 | Golden generation fixtures (`query` half-open rational spans, stable IDs) | Same seed/state/span → bit-identical event IDs/fields across hosts |
| C2 | Musical events carry continuous pitch (cents/Hz), energy, gate, expression; note-off / all-notes-off / scheduled params | Schema validation + fixture coverage |
| C3 | DSP `prepare`/`render`/`reset`: caller buffers, sample-offset events, no alloc/locks/I/O in render | RT checklist + fixture render |
| C4 | Rate/channel negotiate; spatial is adapter; capabilities ≠ Pi caps | Contrail can advertise spatial/stereo; ZPOD can advertise mono/stereo subset |
| C5 | Input begin/update/end with monotonic timestamps, source/contact IDs; generators see no raw HID | Adapter unit tests |
| C6 | Parameter descriptors + versioned presets; visualization snapshots; core excludes UI/MPD/Wi-Fi | Grep/architecture check + schema tests |
| C7 | Contrail Swift engines not required to rewrite to Rust | Review gate: schemas/fixtures sufficient for Orrery + Mouse Music needs |
| C8 | Buffering latency vs measured E2E latency reported separately | Bench docs show both columns |
| U1 | Gadget mode blocks host tools with explicit `unsupported` | Message names role conflict |
| U2 | Host mode enumerates test HID | `lsusb` + events |
| U3 | Hotplug HID | Add/remove OK |
| W1 | Wi-Fi PSK not in journal/logs | `journalctl` scrub check |
| W2 | Assoc + IP verified before role switch | Switch gated |
| W3 | Deploy via configurable host | Flag/env works for Wi-Fi IP |
| G1 | ND-105C detect | Device present state |
| G2 | NMEA fix display | Crisp lat/lon/time |
| G3 | Sat/SNR | Bars or honest “n/a” if sentences lack SNR |
| G4 | State matrix | no-device / no-fix / fix / stale / error / unsupported |
| M1 | Mouse Music buttons-only | Still fully playable |
| M2 | Mouse Music USB mouse path | When host+HID available |
| A1 | Audio adapter isolation | Core has no MPD symbols |
| S1 | Secrets | No SSID/PSK literals in this spec or committed configs |

---

## 12. Risks and open questions

| Risk / question | Mitigation |
| --- | --- |
| Single USB port role flapping bricks Mac workflow | Wi-Fi verify-first; retain documented gadget restore; configurable endpoints |
| PL2303 counterfeit / clone quirks | VERIFY-ON-DEVICE; keep raw NMEA path; gpsd optional |
| Non-unique USB IDs | Prefer interface class + user selection if multiple ttyUSB |
| Pi Zero host performance with hubs | Test real hub; powered hub if flaky |
| Buildroot size for HID/gpsd | Prefer minimal in-app NMEA reader for round 1 if gpsd is heavy |
| Contrail API mismatch | Schemas + golden fixtures first; optional C ABI later; keep UI separate |
| Forcing Rust rewrite of Contrail Swift | Explicitly rejected; map Orrery/Swift engines via schemas |
| Confusing buffer math with E2E latency | Separate metrics (§4.12) |
| “Fat synthesis” CPU on ARMv6 | Quality tiers + platform capability ads; measure with existing `--bench` intent |
| BT timeline | Keep explicitly later; USB path unblocks Wi-Fi typing first |

**Open decisions (do not silently assume):**

1. Reboot vs live `dr_mode` switch for production UX.
2. gpsd vs dedicated NMEA crate for round 1.
3. Whether GPS reader is a daemon exporting a socket or in-process in the tool.
4. Schema repo location and ownership (ZPOD repo, Contrail repo, or neutral shared package).
5. Whether/when to adopt the optional versioned C ABI vs fixtures-only + parallel Swift/Rust implementations.
6. Exact crate/module layout for ZPOD Rust vs Contrail Swift adapters.
7. Whether Wi-Fi tool lives in Settings or Tools submenu.
8. Formal acceptance of Contrail-proposed contracts (§4.4–§4.11) by Paul — currently **proposed only**.

---

## 13. Deliverables checklist for future implementation PRs (not done now)

- [ ] Language-neutral schemas + golden event/render fixtures (Contrail proposals §4.4–§4.11)
- [ ] Portable core split PR(s) on ZPOD (Rust) aligned to schemas; MPD/ALSA remain adapters
- [ ] Contrail Swift engine mapping spike (Orrery modal/strikes/polymeter/spatial; rational patterns) — **no mandated Rust rewrite**
- [ ] Optional versioned C ABI spike only after fixtures
- [ ] USB role manager + host image/config variant
- [ ] Mouse Music input startup + HID adapter → begin/update/end
- [ ] Wi-Fi client entry tool (secret-safe)
- [ ] Deploy script endpoint configuration
- [ ] GPS ND-105C tool + tests
- [ ] Update in-tree docs (`USB-GADGET.md`, apps roadmap) **when implementing** — out of scope for this capture file alone

---

## 14. Concise findings

1. **Portability** means shared **schemas/contracts** + per-platform engines/UI/adapters; `.zpa` is ZPOD packaging only; same binary everywhere is false.
2. **Contrail proposals** (§4.4–§4.11): deterministic `query(start,end,seed,state)` in rational musical time; rich events (continuous pitch, energy, gate, expression); DSP `prepare`/`render`/`reset` with caller buffers and no RT alloc/locks/I/O; input begin/update/end; versioned parameters/presets; schemas + golden fixtures first; optional C ABI later; **do not** mandate Swift→Rust rewrite.
3. **Mouse Music** needs harmony/voicing; **Contrail Orrery** needs modal synthesis, timed strikes, polymeter, spatial positions — same contracts, different engines.
4. Keep **buffering latency** distinct from **measured end-to-end latency**.
5. **USB** on Pi Zero W is mutually exclusive host vs CDC-NCM gadget; Wi-Fi client (keyboard-entered PSK, no secret logs) unlocks host peripherals.
6. **GPS first** after HID+Wi-Fi: ND-105C ≈ PL2303 + NMEA 0183 4800 8N1; exact MTK PN **VERIFY**; round 1 = detection + crisp fix/sat UI + honest states.
7. **This file is requirements + plan only** — Contrail contracts are **proposed**, not approved implementation, and not claimed verified as existing shipping code.

---

## 15. Exact path and source links

**Spec path:** `/workspace/zpod-plans/portable-apps-usb-gps-2026-11-05.md`

**Sources:**

- https://www.jfcarter.net/~jimc/documents/gps-clock-1507.html
- https://www.globalsat.ru/Sites/global/Uploads/ND-105C_User_Manual_Rus.0FDB23A531AC4CCC9B7D6A71B5B50EC6.pdf
- https://github.com/torvalds/linux/blob/master/drivers/usb/serial/pl2303.h
- https://gpsd.io/troubleshooting.html
- https://pip-assets.raspberrypi.com/categories/685-app-notes-guides-whitepapers/documents/RP-009276-WP-1-Using-OTG-mode-on-Raspberry-Pi-SBCs
- https://forums.raspberrypi.com/viewtopic.php?t=347847
- https://forums.raspberrypi.com/viewtopic.php?t=155432 (ND-105C on Pi discussion)
- In-repo references (read-only for this pass): `docs/USB-GADGET.md`, `docs/BOOT-WIFI.md`, `docs/APP-BUNDLE.md`, Mouse Music `DESIGN.md`
- Contrail-proposed contracts: steering handoff from Contrail (2026-11-05); recorded in §4.4–§4.11 as **proposed only**

---

*End of specification. Status remains: captured plan + proposed contracts, not implemented / not approved as shipping code.*
