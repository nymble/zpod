# zpod-midi — MIDI output library

`apps/crates/zpod-midi` sends any app's note events to a MIDI port. Pipit
uses it from 0.3. MouseMusic and Tidepool can use it later; they are **not**
changed yet (see Follow-up).

## Pieces

| Module | Thread | What it does |
|---|---|---|
| `config` | UI | `MidiConfig`, and the strict `zpod-midi` v1 TOML file |
| `router` | audio (render) | events → timed messages. Note-lifetime table, retrigger, panic, per-channel bend, opt-in clock. No allocation, locks or IO. |
| `queue` | render → worker | bounded SPSC ring (1024), allocated once. Full = drop + count + `lost` flag. |
| `out` | worker (nice +5) | writes at the due time, retries a busy port for 20 ms, hotplug rescan every 1 s, cleanup, `Status` counters |
| `sink` | worker | `AlsaOpener` (rawmidi char devices, non-blocking), `FakeOpener` / `FakeSink` for tests |
| `encode` | any | MIDI 1.0 bytes, `bend_value`, `vel7` |

## Using it from an app

```rust
let out = MidiOut::start(Box::new(AlsaOpener::default()));
out.set_wanted(cfg.route.sends(), cfg.port);          // UI thread
let mut router = Router::new(out.ring.clone(), cfg, SR, LATENCY_NS);
// audio thread, once per block:
router.block(now_ns, frames, &events, tempo_bpm, playing);
let internal = router.config().internal_audible(out.connected());
// on exit: stop audio first, then out.shutdown()   (drains and cleans up)
```

`In::On { lane, id, note, vel }` takes a fractional MIDI note. Lanes map to
channels, with an optional fixed note, through `MidiConfig::lanes`.

## File format (`midi.toml`)

```toml
format = "zpod-midi"
version = 1
route = "internal"      # internal | midi | both
fallback = "internal"   # internal | silent   (route = midi without a port)
port = "auto"           # or a port label / "hw:C,D", up to 32 ASCII chars
clock = false           # MIDI Clock + Start/Stop out
bend_range = 2          # 1..24 semitones, must match the receiver
channels = [10, 10, 10, 2, 3]   # one per lane, 1..16
notes = [36, 38, 42, -1, -1]    # fixed note 0..127, or -1 = the lane's pitch
```

Every key is required; unknown keys, wrong lengths and out-of-range values
are rejected. Apps fall back to Internal on any error.

## Guarantees and limits

* **Never on by itself.** The default is Internal. `Unavailable` is
  reported, never shown as connected.
* **Notes always end.**
  * A retrigger sends note-off before note-on.
  * A stale note-off sends nothing.
  * Panic on routing change, AllOff, stop and exit.
  * The worker keeps its own table of notes it actually wrote, and ends them
    after drops, errors, a reconnect and at shutdown.
* **Clock:** 24 ppqn, strictly increasing due times, spacing follows tempo
  from the next pulse. Start comes before the first note, Stop on transport
  stop or when the clock is disabled.
* **Pitch bend** is per channel. This is exact when the notes on a channel
  share one detune. There is no MPE.
* **ALSA:** documented kernel rawmidi devices only (`/dev/snd/midiC*D*`,
  names from `/proc/asound`). No alsa-lib, sequencer client or daemon. A
  busy port never blocks the render thread.
* **Tested:**
  * byte-exact unit tests with the fake sink (14), and the Pipit
    `--midi-test`;
  * a FIFO through the real writer on the device.
* **Not tested:** real rawmidi ports and external synths.

## Follow-up (not done)

* **MouseMusic and Tidepool:** add a `MidiOut` and a `Router` beside their
  engines, convert their events to `In`, and add the same Route / Device /
  Channel UI. No library changes are expected.
* **MIDI input** from Trill or keyboards: a separate reader, not part of
  this library yet.
* **Composite NCM + MIDI gadget activation:** see PIPIT-DESIGN.md.
