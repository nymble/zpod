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

Not in 0.1.0 (see [Deferred](#deferred)): song chaining, live recording,
MIDI, sample playback, a voice editor screen, and running at the same time
as other audio apps.

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

## Pattern file (`pipit-pattern` v1)

```toml
format = "pipit-pattern"
version = 1
name = "Night Drive"   # 1-24 chars
tempo = 104.0          # 50..200 bpm (UI: 60..180)
swing = 54             # 50..75 %
length = 16            # 1..16 steps
scale = "minor"        # minor major dorian phrygian penta-minor penta-major blues
root = 9               # 0..11 (C..B)
seed = 1               # chance hashing

[[track]]              # exactly 5, in order KICK SNARE HAT BASS LEAD
voice = "kick.deep"    # voice id in the bank
level = 100            # 0..127
mute = false
pitched = false        # true: note = scale degree; false: semitones from the voice's pitch
octave = 3             # 0..7 (UI: 1..6), pitched tracks only
on   = [1,0,0,0, 1,0,0,0, 1,0,0,0, 1,0,0,0]   # 16 values each
note = [0,0,...]       # -14..21
vel  = [100,...]       # 1..127 (110+ is drawn as an accent)
gate = [2,...]         # quarter-steps 1..16
prob = [100,...]       # 0..100 %
```

* Unknown keys, a wrong array length and out-of-range values are rejected with
  a message, and the file is not loaded.
* Pitched tracks compute `midi = 12*(octave+1) + root + scale(note)`. A degree
  past the end of the scale wraps into the next octave.
* Drum tracks play `pitch_of_voice * 2^(note/12)`.

## Voice bank (`pipit-voices` v1)

Packaged as `voices/default.toml` and validated when the app starts. If the
file is missing or invalid, the app logs why and uses the built-in copy.
A test checks that the packaged file and the built-in copy are identical.

```toml
format = "pipit-voices"
version = 1
[[voice]]
id = "bass.acid"     # 1-24 chars a-z 0-9 . - _ ; unique
name = "Acid Bass"
kind = "mono"        # kick snare hat clap mono poly pluck fm
# numeric fields (all optional; default, min..max):
```

| field | default | range | meaning |
|---|---|---|---|
| level | 0.8 | 0–1.5 | voice gain |
| pitch | 100 | 20–4000 Hz | drum body pitch at C4 |
| sweep / sweep_time | 1 / 0.03 | 1–10 / 0.001–0.5 s | kick/snare pitch drop |
| attack, decay, sustain, release | .003, .3, .7, .1 | s / 0–1 | ADSR (drums: decay) |
| tone | 0.5 | 0–1 | click / brightness |
| noise | 0.5 | 0–1 | noise mix (snare, clap, hat) |
| cutoff, reso | 2000, 0.2 | 30–18000 Hz, 0–0.95 | state-variable filter |
| env_amt | 0.3 | 0–1 | filter envelope depth |
| drive | 0 | 0–1 | soft saturation |
| glide | 0 | 0–1 s | mono portamento (legato) |
| detune | 0 | 0–50 cents | second oscillator |
| wave | 0 | 0–1 | saw → square |
| ratio, index | 1, 1 | 0.25–16, 0–12 | FM modulator ratio / depth |

Voice kinds (all synthesized; no samples, so there is nothing to license):

* **kick:** sine body with an exponential pitch sweep, a click transient and
  drive.
* **snare:** two tuned body partials, plus band-passed noise.
* **clap:** three quick noise bursts with a tail.
* **hat:** high-passed noise and metallic square partials. Closed and open hats
  share one choke group.
* **mono:** polyBLEP saw/square into a resonant TPT state-variable low-pass,
  with a filter envelope, legato and glide.
* **poly:** as mono, but up to 4 notes with a detuned second oscillator.
* **pluck:** Karplus-Strong string with a damping filter (tone).
* **fm:** 2-operator FM (bell or keys).

Default bank, 12 voices: `kick.deep`, `kick.tight`, `snare.crack`, `clap`,
`hat.closed`, `hat.open`, `bass.round`, `bass.acid`, `lead.pluck`, `lead.bell`,
`lead.keys`, `lead.square`. In the TRACK screen, a drum lane can choose only
drum kinds and a tuned lane only tuned kinds.

### Clean output

* **Envelopes:** attacks are at least 16 samples, and every attack starts
  from the current level.
* **Retrigger and voice stealing:** the old voice's last output is carried
  over and decays (×0.94 per sample) under the new one, so the waveform has no
  step.
* **Silence:** a voice ends when its envelope falls below 1e-4 (−80 dB).
* **Lanes:** each lane has level smoothing and a fixed pan:
  snare +0.08, hat +0.30, lead −0.25, kick and bass centred.
* **Master:** bus gain 0.55, then a DC blocker, then a peak limiter with a
  0.89 ceiling (−1 dBFS), then the smoothed output level (Soft 0.50,
  Medium 0.71, Full 1.0). The default is Soft.

The tests check every voice for DC, jumps and ending. They also check
retriggers for clicks, and that all four demos stay between −30 and −8 dBFS
RMS with no samples over the ceiling and no voices left after Stop.

Measured offline (`cargo run --release -p zpod-voices --example render_demo`,
10 s per demo): peaks −2.3 to −3.6 dBFS, RMS −18.7 to −20.4 dBFS, DC about 0,
0 overs. These are measurements, not a judgement of how the voices sound.

## UI and button mapping

There are nine inputs: arrows, OK, and Triangle/Circle/Square/X, with Play
and Home handled by the system keys.

| Key | Everywhere | GRID | STEP / TRACK / SONG |
|---|---|---|---|
| Play | start/stop (even in menus) | | |
| Arrows (repeat) | | move cursor (wraps) | Up/Down choose param, Left/Right change |
| OK / Circle | | step on/off | STEP: step on/off; TRACK: mute |
| Triangle | next mode | | |
| Square | Actions menu | | |
| X | close | help | back to GRID |
| Home | save and exit | | |

* **Modes:** STEP edits the step under the cursor, TRACK edits the lane, and
  SONG edits the global settings. There are no button chords, so every
  function can be reached one button at a time.
* **Cursor and playhead:** the cursor is a white box, and the playhead is a
  green bar with a column outline. They are never drawn the same way.
* **Changes that replace the pattern** (demo, Euclid, notes, clear, new) are
  only in the Actions menu, and one Undo is kept.
* **Saving over a used slot** needs a second OK.

## Files

`$ZPOD_DATA_DIR/pipit/` (on the device: `/data/zpod/appdata/pipit/`, owned
by zpod):

* `current.toml` is saved automatically 2 s after an edit and on exit.
* `slot-1.toml` … `slot-8.toml` are the saved patterns.
* `output` stores the output level.

A malformed file is reported in a toast or the log, and the pattern in use is
kept.

## CPU budget (ARMv6, Pi Zero W)

`pipit --bench SECS` renders each demo offline and prints the CPU time used
as a percentage of real time. It fails if a demo is over 70 %.
`pipit --selftest SECS` plays the first demo through the real DAC at the Soft
level, then stops. It reports xruns and render CPU. Device numbers are in
[PIPIT.md](PIPIT.md#measurements).

## Deferred

* **Voice bank screen:** loading other `pipit-voices` files from
  `/data/...` and editing the voice fields on the device. The format already
  supports this, but there is no UI.
* **Song mode:** chaining patterns, plus a per-pattern bar count.
* **Live recording:** record while playing, and parameter locks per step
  (beyond note/vel/gate/chance).
* **Concurrent running:** an audio service, a mixer and IPC (see "Routing
  boundary").
* **MIDI:** in/out and clock sync.
* **Samples:** sample playback (it would need clearly licensed sample
  packs).
* **Mute without pausing music:** muting Pipit's audio rather than pausing
  MPD.
