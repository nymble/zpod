# Pipit — design

Pipit is a small step sequencer for the ZPOD. Five 16-step lanes play
synthesized drum and tuned voices. The design takes ideas from pocket-sized
groove boxes: one screen, a few buttons, and instant play. It is not
affiliated with any hardware maker and uses none of their names, sounds or
artwork.

It is a playable instrument, not an ambient generator. Nothing changes the
pattern unless you press a button. The generators (Euclid, new notes) run
only from the Actions menu, are seeded and repeatable, and can be undone.

User manual: [PIPIT.md](PIPIT.md).

## Goals and non-goals

| Goal | How |
|---|---|
| Playable with 9 buttons | Arrow-driven modes; OK toggles a step; Play is always the transport |
| Predictable | Play always starts at step 1; the generators are seeded; probability is a hash of (seed, bar, track, step), so a pattern sounds the same every time it plays |
| Timing | Step times are computed from the step index at sample resolution, so no error accumulates |
| Sound | New procedural voices with clean envelopes, a DC blocker, a limiter and a safe default level |
| Data-driven | Versioned TOML for patterns and voice banks, validated strictly |
| Reusable engine | `zpod-seq` (clock and events) and `zpod-voices` (synthesis) know nothing about the UI or ALSA |

Not in 0.3 (see [Deferred](#deferred)): song chaining, live recording,
per-step parameter locks, MIDI input and sync, sample playback,
a voice editor screen, and running at the same time as other audio apps.

0.2 adds the LIVE screen (tempo, tap, rate, swing, mute/solo), beat rates,
the voice browser with audition, per-lane macros, 8 more voices, drum kits,
an echo and drive bus, velocity-dependent drum timbre, an honest ALSA
underrun counter, and a 3–4× lower CPU load from removing subnormal floats.

## Layers

```
 buttons -> editor.rs (modes, params, actions)  -- Pattern (Copy) + version -->
            ui.rs (320x240 drawing)               Shared (Mutex, try_lock in audio)
                                                         |
 audio thread:  Clock (zpod-seq) --EventBuf--> Rack (zpod-voices) --> f32 stereo
                                                         |
                  out_gain (Soft/Medium/Full) -> i16 -> zpod-music-core AudioRuntime
                  (aplay, hw:CARD=sndrpiaoidezpod, 44.1 kHz, 256-frame periods)
```

* **`apps/crates/zpod-seq`** has the pattern model, the clock, the generators and
  the demo grooves. It is pure Rust with no dependencies on I/O or audio, and
  its tests run on any host.
* **`apps/crates/zpod-voices`** has the voice bank parser and validator, the
  DSP building blocks and the voice rack.
* **`apps/crates/pipit`** is the app: the editor state machine, drawing, files
  and the audio glue. In `zpod-music-core`, `AudioRuntime::start_render` was
  added: a callback renders into a reused buffer, and the existing
  `start_with` now calls it, so the other music apps behave the same.

### Real-time rules

The audio callback runs `Engine::period()`:

* It does not allocate. `EventBuf` is a fixed array of 128 events, the
  note-off queue is a fixed array of 48, and the mix buffer is sized once.
* It does no file I/O. Banks and patterns are parsed on the UI thread before
  they reach the engine.
* It never blocks. It reads the shared pattern with `try_lock`. If the lock
  is busy, it keeps the previous pattern for that 5.8 ms period.

The pattern is a `Copy` struct of about 1 KB. Each edit increases a version
number, and the engine copies the pattern only when that number changes.

## Events and the clock

```rust
struct Pitch { hz: f32 }                    // from_note(f32) allows cents
enum EventKind {
    NoteOn  { track: u8, id: u32, pitch: Pitch, vel: u8 },
    NoteOff { track: u8, id: u32 },
    AllNotesOff,
}
struct Event { offset: u32 /* frame in this block */, kind: EventKind }
```

* **Note ids** are unique for each note-on (a counter). A `NoteOff` names the id
  that it ends, so a voice that was stolen or retriggered is never cut off by a
  stale off.
* **Step times:** `frame(step n) = anchor + n * step_len`, plus swing. Each
  start time is computed from the step index, not by adding step lengths, so
  there is no drift (tested over 10 minutes against the closed form). Changing
  the tempo re-anchors the clock at the next step.
* **Swing** delays odd steps (2, 4, …) by `(swing/50 - 1) * step_len`. Swing 50
  is straight; 66 is close to a triplet feel; the maximum is 75.
* **Gate** is in quarter-steps (1..16, so up to 4 steps). The clock schedules the
  note-off together with the note-on. Every note-on has exactly one note-off.
  If the off queue is full, the oldest note ends early.
* **Probability:** a step plays when `hash(seed, bar, track, step) % 100 < prob`.
  The pattern's `seed` makes this repeatable.
* **Start** always plays from step 1 at the next block. **Stop** sends every
  pending off and then `AllNotesOff`. The rack then lets the release tails
  finish, so they fade without a click.
* **Block size:** splitting the same audio into blocks of 1..1024 frames gives
  identical events and identical audio (tested).

### Routing boundary for future concurrent apps

Pipit owns the DAC while it runs; it pauses MPD on entry and leaves it paused
on exit, like the other music apps. **It is not yet a shared service.** The
clock produces `Event`s with frame offsets and stable track/note ids, and
anything that consumes `&[Event]` can render them. That is the seam where a
future arrangement could put the clock and rack in a small audio service with
a mixer, so another app could play over the same clock. Those apps would need
an IPC transport carrying the pattern and transport commands. That service,
its IPC and any mixing are **not built**.

## Pattern file (`pipit-pattern` v2)

```toml
format = "pipit-pattern"
version = 2            # 1 is still accepted (see Migration)
name = "Night Drive"   # 1-24 chars
tempo = 104.0          # 50..200 bpm (UI: 60..180)
swing = 54             # 50..75 % (ignored at rate 1/16t)
length = 16            # 1..16 steps
scale = "minor"        # minor major dorian phrygian penta-minor penta-major blues
root = 9               # 0..11 (C..B)
seed = 1               # chance hashing
rate = "1/16"          # v2: "1/8" "1/16" "1/32" "1/16t" (2, 4, 8, 6 steps per beat)
drive = 0              # v2: 0..10 master saturation
echo = 0               # v2: 0..10 echo return
echo_time = 1          # v2: 0..3 = 1/8, 3/16, 1/4, 3/8 note

[[track]]              # exactly 5, in order KICK SNARE HAT BASS LEAD
voice = "kick.deep"    # voice id in the bank
level = 100            # 0..127
mute = false
solo = false           # v2
pitched = false        # true: note = scale degree; false: semitones from the voice's pitch
octave = 3             # 0..7 (UI: 1..6), pitched tracks only
tone = 0               # v2: -10..10
decay = 0              # v2: -10..10
pitch = 0              # v2: drums -12..12 semitones; pitched -50..50 cents
drive = 0              # v2: 0..10
send = 0               # v2: 0..10 echo send
on   = [1,0,0,0, 1,0,0,0, 1,0,0,0, 1,0,0,0]   # 16 values each
note = [0,0,...]       # -14..21
vel  = [100,...]       # 1..127 (110+ is drawn as an accent)
gate = [2,...]         # quarter-steps 1..16
prob = [100,...]       # 0..100 %
```

* **Validation:** unknown keys, a wrong array length and out-of-range values
  are rejected with a message, and the file is not loaded.
* **Pitch:** pitched tracks compute
  `midi = 12*(octave+1) + root + scale(note) + pitch/100`. A degree past the
  end of the scale wraps into the next octave.
* **Drum tracks** play `pitch_of_voice * 2^((note + pitch)/12)`.
* **Audible lanes:** a track plays when `!mute && (no track is soloed ||
  solo)`.

### Migration

Every v2 field has a serde default equal to 0.1 behaviour: rate 1/16,
echo_time 1, and all others 0/false. So a v1 file loads with identical
sound, and the next save writes v2. Nothing is ever dropped or silently
replaced.

* **Test fixture:** `zpod-seq/testdata/pattern-v1.toml` is the real
  `current.toml` read from the device running 0.1.1.
* **On the device (0.1.1 → 0.2.0):**
  * `zpod-app update` left the file byte-identical (same sha256).
  * After launch and stop, `diff` against the v1 backup shows only
    `version = 2` and the added default lines.

## Clock, rates and live changes

* `step_frames = sr * 60 / (bpm * steps_per_beat)`.
* Step `n` starts at `anchor + (n - anchor_step) * step_len + swing(n)`,
  computed from the index, so nothing drifts.
* **Tempo or rate change while playing:** the clock re-anchors at the next
  step. The anchor is clamped to "now", so a step is never scheduled in the
  past and nothing replays.
  * Note-offs already scheduled stay where they are, so there are no stuck
    notes.
  * Tests cover tempo ×2/÷2 and rate changes mid-run: steps stay in order,
    and every note-on gets its note-off.
* **Triplet rate** (6 steps per beat) has no swing.
* **Control latency:** Pipit sets the aplay pipe to 16 KiB, which is about
  93 ms. The music-core default is 64 KiB, about 370 ms, because a blocking
  writer keeps the pipe full. Add the 17 ms ALSA buffer.
  `music_core::PIPE_BYTES` is opt-in, so the other apps are unchanged.

## UI and button mapping

There are nine inputs: arrows, OK, and Triangle/Circle/Square/X, with Play
and Home handled by the system keys.

| Key | Everywhere | LIVE | GRID | STEP / TRACK / SONG |
|---|---|---|---|---|
| Play | start/stop (menus and browser too) | | | |
| Arrows (repeat) | | Up/Down row; Left/Right value. Lanes: Left = mute, Right = solo (press only) | move cursor (wraps) | Up/Down param (pages of 6), Left/Right value |
| OK / Circle | | Tempo row: tap; lane: mute toggle | step on/off | STEP: step on/off; TRACK: Voice → browser, Mute row → toggle |
| Triangle | next mode (LIVE first) | | | |
| Square | Actions menu | | | |
| X | close | help | help | back to GRID |
| Home | save and exit | | | |

Design rules:

* **Tempo row:** tap is the only OK action there, so editing tempo can never
  mute a lane.
* **Lane rows:** OK only toggles mute, and a lane state never changes on key
  repeat.
* **Tap:** a 5-timestamp ring averages up to 4 intervals.
  * 120 ms debounce; a gap over 2 s resets the count.
  * The result is clamped to 60–180 bpm and rounded to whole bpm.
  * Time comes from a monotonic clock read just before each key is handled.
* **Voice browser:** audition happens only on an explicit Right press.
  Moving through the list is silent.
  * The audition plays on a separate slot (`AUDITION`, index 5), so it
    neither steals nor changes a lane voice.
  * It is a 0.7 s note: drums at their own pitch, tuned voices at the key
    root in the lane's octave, with that lane's macros.
* **Changes that replace things** (demo, kit, Euclid, notes, clear, new) are
  only in the Actions menu, with one Undo. Saving over a used slot needs a
  second OK.

## Voice bank (`pipit-voices` v2)

Packaged as `voices/default.toml` and validated when the app starts. If the
file is missing or invalid, the app logs why and uses the built-in copy,
which a test keeps identical.

* **v2 additions:** each voice may carry a `category` (1–10 chars a-z, the
  browser group) and a `desc` (up to 34 printable ASCII characters).
* **v1 banks still load.** Their voices get a category from their kind.
* `name` is limited to 24 printable ASCII characters.

```toml
format = "pipit-voices"
version = 2
[[voice]]
id = "bass.acid"     # 1-24 chars a-z 0-9 . - _ ; unique
name = "Acid Bass"
category = "bass"    # v2, optional
desc = "resonant squelch bass with glide"   # v2, optional
kind = "mono"        # kick snare hat clap mono poly pluck fm
```

| field | default | range | meaning |
|---|---|---|---|
| level | 0.8 | 0–1.5 | voice gain |
| pitch | 100 | 20–4000 Hz | drum body pitch at C4 |
| sweep / sweep_time | 1 / 0.03 | 1–10 / 0.001–0.5 s | kick/snare pitch drop |
| attack, decay, sustain, release | .003, .3, .7, .1 | s / 0–1 | ADSR (drums: decay) |
| tone | 0.5 | 0–1 | click / brightness / metal vs noise (hat) |
| noise | 0.5 | 0–1 | noise mix (snare, clap, hat) |
| cutoff, reso | 2000, 0.2 | 30–18000 Hz, 0–0.95 | state-variable filter |
| env_amt | 0.3 | 0–1 | filter envelope depth |
| drive | 0 | 0–1 | soft saturation |
| glide | 0 | 0–1 s | mono portamento (legato) |
| detune | 0 | 0–50 cents | second oscillator |
| wave | 0 | 0–1 | saw → square |
| ratio, index | 1, 1 | 0.25–16, 0–12 | FM modulator ratio / depth |

### Provenance

Every voice is procedural: the oscillators, noise and filters are driven by
the numbers above.

* There are **no samples, recordings or third-party presets**, and no
  hardware maker's sounds or names are used.
* The 20 voice designs (12 from 0.1, 8 new in 0.2) were written for this
  project by its author.

### Kinds

* **kick:** a sine body with an exponential pitch sweep.
  * Velocity deepens the sweep (`0.55 + 0.45·v`).
  * A band-passed noise click opens with velocity (`cutoff·1.6·(0.7 + 0.5·v)`).
  * Then drive. Toms are kicks with a higher pitch and a short decay.
* **snare:** two tuned body partials and band-passed noise, plus a
  high-passed "snap" layer.
  * The noise filter follows velocity (`cutoff·(0.55 + 0.7·v)`).
  * The snap is v² with a 7 ms decay.
  * The noise mix rises with velocity.
* **clap:** three noise bursts with a tail; the band-pass follows velocity.
* **hat:** six metallic square partials, band-limited with polyBLEP, plus
  noise, then high-pass and low-pass.
  * Closed and open hats share a choke group.
  * `tone = 0` gives a pure-noise shaker.
* **mono / poly / pluck / fm:** as in 0.1. These are a polyBLEP saw/square
  into a TPT SVF with glide; 4 detuned voices; Karplus-Strong; and
  2-operator FM. `pad.glass` is a slow poly.

### Macros

Macros are applied at note-on (`zpod_voices::shape`). They never change the
bank; they scale a copy of the voice definition.

| Macro | Effect |
|---|---|
| tone *t* | cutoff × 2^(t/6), tone + 0.04·t |
| decay *d* | decay and release × 2^(d/6) |
| drive *m* | drive + 0.08·m |

Results are clamped to the bank ranges. Pitch is applied by the pattern
(see Pattern file). Tests cover every voice at every macro extreme: finite
output, bounded, longer with +decay, brighter with +tone.

### Master bus

Signal order: lanes (level smoothing, fixed pans) → [echo] → DC blocker →
[drive] → peak limiter (ceiling 0.89, −1 dBFS) → smoothed output level
(Soft 0.50, Medium 0.71, Full 1.0).

* **Echo:** stereo ping-pong with a 2 s buffer, preallocated once.
  * Fixed feedback is 0.42, through a one-pole damping low-pass, so it always
    decays.
  * The time follows tempo.
  * A time change fades the return out, wipes the buffer 3 frames per sample
    (no CPU spike) and fades back in.
* **Drive:** `sat(y·BUS·g)·n`, with gain compensation, before the limiter.

The real-time rules are unchanged:

* no allocation, IO or locks in render;
* bounded polyphony and tails;
* every attack starts from the current level, and steals cross-fade.

### Subnormal floats (CPU)

The ARM1176 VFP on the Pi Zero runs subnormal arithmetic about 100× slower.
Fast click, snap and sweep envelopes, and the echo, DC-blocker and limiter
states, decay into that range while a long body is still ringing.

* **Measured:** a long kick took 49 % of a core; the Boom Trip demo took
  82 %, with 42 real underruns.
* **Fix:** each block zeroes voice envelopes below 1e-9 and master states
  below 1e-12 (−180 dB or less, inaudible). Echo writes below 1e-12 are
  stored as 0.
  * `music_core::flush_denormals()` also sets FPSCR flush-to-zero and
    default-NaN on the audio thread. The fix measured the same with and
    without it.
* **Result:** Night Drive went from 44.6 % to 14.9 %, Boom Trip from 82 %
  to 21.4 %.

## Diagnostics: underruns vs write errors

`aplay -q` still writes `underrun!!!` to stderr. Before 0.2, music-core piped
that stderr without reading it. That never showed the underruns, and in
theory it could fill the pipe and block aplay.

Now music-core drains aplay's stderr on its own thread, which:

* counts lines containing "underrun" or "overrun" into
  `AudioRuntime::alsa_underruns`;
* prints up to 20 other lines.

| Counter | Meaning |
|---|---|
| `alsa_underruns` | real DAC starvation, reported by aplay |
| `xruns` / write errors | failed writes into the pipe. These include the teardown after a group SIGTERM, when aplay dies first. |

Test commands:

* `pipit --selftest S [DEMO]` fails if either counter is not 0.
* `pipit --xrun-test` proves the counter works: it plays silence, stalls for
  1 s (longer than the pipe and buffer) and requires at least 1 underrun.

## Files

`$ZPOD_DATA_DIR/pipit/` (on the device: `/data/zpod/appdata/pipit/`, owned
by zpod):

* `current.toml` is saved automatically 2 s after an edit and on exit.
* `slot-1.toml` … `slot-8.toml` are the saved patterns.
* `output` stores the output level.

A malformed file is reported in a toast or the log, and the pattern in use is
kept.

## CPU budget (ARMv6, Pi Zero W)

* `pipit --bench SECS` renders the 5 demos, plus Night Drive with drive,
  echo and every send at 10, offline. It fails if any run is over 70 % of
  one core.
* `pipit --bench SECS DEMO` breaks one demo down: the whole demo, each lane
  soloed, without echo, and at rate 1×.
* `pipit --selftest SECS [DEMO]` plays a demo through the DAC at Soft for a
  bounded time.

Device numbers are in [PIPIT.md](PIPIT.md#measurements).

## MIDI output (0.3)

Library: `apps/crates/zpod-midi`, reusable, see [ZPOD-MIDI.md](ZPOD-MIDI.md).
It depends on no app, UI or audio code.

* **Pipit hookup:** after `clock.process`, the engine converts its events
  (except the audition) into `zpod_midi::In` and calls `Router::block`.
  * The rack renders the full events when
    `config.internal_audible(connected)` is true.
  * Otherwise it renders only the audition, and one AllNotesOff when the
    internal sound turns off.
* **Config:** the UI thread owns `MidiConfig` and passes it through
  `Shared` with a version number. The router applies it at the next block
  and panics first if routing changed.
* **Files:** `midi.toml` sits beside the patterns and is written only when
  the setup closes. Patterns are not touched; pattern v1 and v2 load as
  before.
* **Defaults:** route Internal; fallback internal; port auto; clock off;
  bend ±2; channels 10/10/10/2/3; drum notes 36/38/42.
  * A missing or invalid file means Internal, with a toast.
  * Nothing switches the route by itself.
* **Pitch:** fixed-note drum lanes ignore pitch. Tuned lanes send the
  nearest note plus a per-channel 14-bit bend for the cents macro.
  * This is exact because the detune is per lane and each lane has its own
    channel. Lanes that share a channel share one bend.
  * Polyphonic per-note detune would need MPE, which is not implemented and
    not claimed.
* **Latency:** MIDI is due 110 ms after the block, the estimated render lead
  (pipe plus ALSA buffer). This is not measured against hardware.
* **Not built:**
  * MIDI input
  * external clock sync
  * Song Position Pointer
  * per-step parameter locks
  * MPE

### USB composite network + MIDI (staged, not active)

`overlay/usr/lib/zpod/zpod-usb-gadget.sh` (image side) accepts `midi=on` in
`/boot/firmware/zpod-usb.txt`. With it, the script adds `functions/midi.usb0`
(1 in, 1 out port) beside `ncm.usb0`, and sets the product name to "ZPOD USB
Ethernet + MIDI".

* Without that line the gadget is exactly as before, and `tests/usb-gadget-midi.sh`
  checks this in a fake tree: no configfs, nothing bound.
* The running device's gadget was not touched.

Activating it is **pending** and needs:

1. A system image or core update that ships the new script. The installed
   test6 image has the old one.
2. An on-device GUI switch in Tools → USB to write `midi=on` and rebuild the
   gadget the same way the live role switch does. Until that exists,
   enabling it is a manual edit of `zpod-usb.txt` followed by a role switch
   or reboot done on the device.
3. A cable test with macOS. It is untested whether macOS keeps the NCM link
   up with the extra MIDI interface, which is why the mode is opt-in.

In host mode no change is needed: a class-compliant MIDI device appears as
`/dev/snd/midiC*D*`, and Pipit lists it.

## Deferred

* **MIDI:** input, external sync, the composite activation GUI (above), and MouseMusic/Tidepool hookup.
* **Per-step parameter locks** and live recording.
* **Song mode:** pattern chaining and a per-pattern bar count.
* **Voice bank screen:** loading other `pipit-voices` files and editing the
  voice fields on the device.
* **Concurrent running:** an audio service, a mixer and IPC (see "Routing
  boundary").
* **Samples:** sample playback (it would need clearly licensed packs).
