//! Exclusive ALSA output for generative music apps + MPD pause/restore.
//!
//! Opens the PCM5122 via `aplay` (raw S16_LE) so the static musl app does not
//! need to link libasound. Period 256 / buffer 768 at 44.1 kHz ≈ 17 ms.
//! On start: remember MPD state, `pause`, `disableoutput 0`. On stop: fade,
//! close PCM, `enableoutput 0`, leave MPD paused (never resume).

use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::harmony::{Chord4, Mapper, Pattern};
use crate::synth::{Engine, DEFAULT_SR, MAX_VOICES};

pub const SAMPLE_RATE: u32 = 44_100;
pub const PERIOD_FRAMES: usize = 256;
pub const BUFFER_PERIODS: usize = 3;
pub const CHANNELS: usize = 2;

const CARD_DEV: &str = "hw:CARD=sndrpiaoidezpod,DEV=0";

/// File the buttons daemon watches for per-app input mode.
pub const BUTTONS_MODE_PATH: &str = "/run/zpod/buttons.mode";
pub const BUTTONS_MODE_FAST: &str = "fast-arrows\n";

#[derive(Debug)]
pub enum AudioError {
    Spawn(String),
    Write(String),
    Mpd(String),
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioError::Spawn(s) | AudioError::Write(s) | AudioError::Mpd(s) => write!(f, "{s}"),
        }
    }
}

/// Take the DAC from MPD. Returns whether MPD was playing and whether output was disabled.
pub struct MpdExclusive {
    pub was_playing: bool,
    pub output_off: bool,
}

impl MpdExclusive {
    pub fn acquire() -> Self {
        let mut was_playing = false;
        let mut output_off = false;
        match zpod_mpd::Client::connect_default() {
            Ok(mut c) => {
                if let Ok(st) = c.status() {
                    was_playing = st.state == "play";
                }
                let _ = c.simple("pause 1");
                output_off = c.simple("disableoutput 0").is_ok();
                eprintln!(
                    "music-core: MPD pause; disableoutput={} (was_playing={was_playing})",
                    output_off
                );
            }
            Err(e) => eprintln!("music-core: MPD not available ({e}); continuing"),
        }
        Self {
            was_playing,
            output_off,
        }
    }

    /// Re-enable the MPD output; leave playback paused (Paul 2026-11-05).
    pub fn release(self) {
        if self.output_off {
            match zpod_mpd::Client::connect_default() {
                Ok(mut c) => {
                    let _ = c.simple("enableoutput 0");
                    // Explicitly stay paused even if something else toggled it.
                    let _ = c.simple("pause 1");
                    eprintln!(
                        "music-core: MPD enableoutput; left paused (was_playing={})",
                        self.was_playing
                    );
                }
                Err(e) => eprintln!("music-core: MPD restore failed: {e}"),
            }
        }
    }
}

/// Request fast D-pad arrows from zpod-buttons (no 50 ms centre-combo wait).
pub struct FastArrowsGuard;

const BUTTONS_MODE_FALLBACK: &str = "/tmp/zpod-buttons.mode";

impl FastArrowsGuard {
    pub fn acquire() -> Self {
        let _ = std::fs::create_dir_all("/run/zpod");
        // Prefer /run/zpod/buttons.mode (daemon makes it 0666); fall back to /tmp.
        let path = if std::fs::write(BUTTONS_MODE_PATH, BUTTONS_MODE_FAST).is_ok() {
            BUTTONS_MODE_PATH
        } else if std::fs::write(BUTTONS_MODE_FALLBACK, BUTTONS_MODE_FAST).is_ok() {
            BUTTONS_MODE_FALLBACK
        } else {
            eprintln!("music-core: could not request fast-arrows (no writable mode file)");
            return Self;
        };
        eprintln!("music-core: requested fast-arrows via {path}");
        Self
    }
}

impl Drop for FastArrowsGuard {
    fn drop(&mut self) {
        for path in [BUTTONS_MODE_PATH, BUTTONS_MODE_FALLBACK] {
            match std::fs::write(path, "") {
                Ok(()) => {
                    // Prefer truncate over remove so a 0666 placeholder stays.
                    let _ = std::fs::remove_file(path);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => eprintln!("music-core: clear fast-arrows {path}: {e}"),
            }
        }
        eprintln!("music-core: cleared fast-arrows mode");
    }
}

/// Requested size of the pipe into aplay, in bytes; 0 keeps the kernel
/// default (64 KiB = ~370 ms of 44.1 kHz stereo, which is also the control
/// latency: a blocking writer keeps the pipe full). An app that wants
/// faster response to tempo or mute changes sets a smaller size before
/// starting audio. Linux rounds up to a page multiple.
pub static PIPE_BYTES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct AplayOut {
    child: Child,
    stdin: ChildStdin,
}

impl AplayOut {
    /// `underruns` counts the "underrun!!!" reports aplay writes to stderr
    /// (it prints them even with -q). stderr is drained by a small thread so
    /// a long run of reports can never fill the pipe and block aplay.
    fn open(underruns: Arc<AtomicU64>) -> Result<Self, AudioError> {
        let mut child = Command::new("aplay")
            .args([
                "-t",
                "raw",
                "-f",
                "S16_LE",
                "-c",
                "2",
                "-r",
                "44100",
                "-D",
                CARD_DEV,
                "--buffer-size=768",
                "--period-size=256",
                "-q",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| AudioError::Spawn(format!("aplay: {e}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AudioError::Spawn("aplay stdin".into()))?;
        let want = PIPE_BYTES.load(Ordering::Relaxed);
        if want > 0 {
            use std::os::fd::AsRawFd;
            // SAFETY: plain fcntl on a pipe fd we own; failure leaves the default.
            let got = unsafe { libc::fcntl(stdin.as_raw_fd(), libc::F_SETPIPE_SZ, want as libc::c_int) };
            eprintln!("music-core: aplay pipe {} bytes (~{} ms)", got, if got > 0 { got as usize * 1000 / (4 * SAMPLE_RATE as usize) } else { 0 });
        }
        // Give aplay a moment; if it dies immediately the device is busy.
        thread::sleep(Duration::from_millis(30));
        if let Ok(Some(status)) = child.try_wait() {
            let err = child
                .stderr
                .as_mut()
                .and_then(|s| {
                    let mut b = String::new();
                    std::io::Read::read_to_string(s, &mut b).ok()?;
                    Some(b)
                })
                .unwrap_or_default();
            return Err(AudioError::Spawn(format!(
                "aplay exited early ({status}): {err}"
            )));
        }
        if let Some(err) = child.stderr.take() {
            let _ = thread::Builder::new().name("aplay-stderr".into()).spawn(move || {
                use std::io::BufRead;
                let mut shown = 0u32;
                for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
                    if line.contains("underrun") || line.contains("overrun") {
                        underruns.fetch_add(1, Ordering::Relaxed);
                    } else if shown < 20 {
                        shown += 1;
                        eprintln!("aplay: {line}");
                    }
                }
            });
        }
        eprintln!(
            "music-core: ALSA open via aplay {CARD_DEV} 44100/S16_LE/2 period=256 buffer=768 (~17 ms)"
        );
        Ok(Self { child, stdin })
    }

    fn write_i16(&mut self, frames: &[i16]) -> Result<(), AudioError> {
        let bytes = unsafe {
            core::slice::from_raw_parts(frames.as_ptr() as *const u8, frames.len() * 2)
        };
        self.stdin
            .write_all(bytes)
            .map_err(|e| AudioError::Write(format!("aplay write: {e}")))
    }

    fn close(mut self) {
        drop(self.stdin);
        let _ = self.child.wait();
    }
}

/// Shared performance state the UI and audio thread both see.
#[derive(Clone, Debug)]
pub struct PerfState {
    pub x: i32,
    pub y: i32,
    pub mapper: Mapper,
    pub pattern: Pattern,
    pub tempo_bpm: u32,
    pub sounding: bool,
    pub chord: Chord4,
    /// Arpeggio step index 0..4
    pub arp_step: u8,
    pub fade: f32,
    pub exit: bool,
}

impl Default for PerfState {
    fn default() -> Self {
        let mapper = Mapper::default();
        let chord = mapper.chord(7, 7);
        Self {
            x: 7,
            y: 7,
            mapper,
            pattern: Pattern::Up,
            tempo_bpm: 112,
            sounding: true,
            chord,
            arp_step: 0,
            fade: 1.0,
            exit: false,
        }
    }
}

pub struct SharedPerf(pub Arc<Mutex<PerfState>>);

impl SharedPerf {
    pub fn new(st: PerfState) -> Self {
        Self(Arc::new(Mutex::new(st)))
    }

    pub fn clone_handle(&self) -> Arc<Mutex<PerfState>> {
        Arc::clone(&self.0)
    }
}

/// Audio thread + exclusive DAC ownership.
pub struct AudioRuntime {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    mpd: Option<MpdExclusive>,
    /// failed writes to aplay (the pipe), e.g. aplay ended; not ALSA xruns
    pub xruns: Arc<std::sync::atomic::AtomicU64>,
    pub frames_written: Arc<std::sync::atomic::AtomicU64>,
    /// ALSA underruns reported by aplay (real dropouts at the DAC)
    pub alsa_underruns: Arc<AtomicU64>,
}

impl AudioRuntime {
    pub fn start(perf: Arc<Mutex<PerfState>>) -> Result<Self, AudioError> {
        let mpd = MpdExclusive::acquire();
        // Small delay so MPD releases the hw device.
        thread::sleep(Duration::from_millis(80));
        let stop = Arc::new(AtomicBool::new(false));
        let xruns = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let frames_written = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let alsa_underruns = Arc::new(AtomicU64::new(0));
        let stop_t = Arc::clone(&stop);
        let xruns_t = Arc::clone(&xruns);
        let frames_t = Arc::clone(&frames_written);
        let under_t = Arc::clone(&alsa_underruns);

        let join = thread::Builder::new()
            .name("zpod-audio".into())
            .spawn(move || {
                flush_denormals();
                if let Err(e) = audio_thread(perf, stop_t, xruns_t, frames_t, under_t) {
                    eprintln!("music-core: audio thread end: {e}");
                }
            })
            .map_err(|e| AudioError::Spawn(format!("audio thread: {e}")))?;

        Ok(Self {
            stop,
            join: Some(join),
            mpd: Some(mpd),
            xruns,
            frames_written,
            alsa_underruns,
        })
    }

    /// Fade out (~50 ms), stop thread, restore MPD output (stay paused).
    pub fn shutdown(mut self) {
        // Caller should set perf.exit/fade beforehand; wait briefly for fade, then stop.
        thread::sleep(Duration::from_millis(60));
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
        if let Some(m) = self.mpd.take() {
            m.release();
        }
        eprintln!(
            "music-core: shutdown; frames={} xruns={} alsa_underruns={}",
            self.frames_written.load(Ordering::Relaxed),
            self.xruns.load(Ordering::Relaxed),
            self.alsa_underruns.load(Ordering::Relaxed)
        );
    }
}

impl AudioRuntime {
    /// Generic audio thread for other generators: `f` runs once per period
    /// (256 frames) before the engine renders it and may send notes or
    /// change `master`; returning false ends the stream (after a short
    /// silent drain). Same MPD pause / exclusive DAC handling as `start`.
    pub fn start_with<F>(mut f: F) -> Result<Self, AudioError>
    where
        F: FnMut(&mut Engine, usize) -> bool + Send + 'static,
    {
        let mut eng = Engine::new(DEFAULT_SR);
        Self::start_render(move |buf, frames| {
            if !f(&mut eng, frames) {
                return false;
            }
            eng.render_i16(buf, frames);
            true
        })
    }

    /// Lowest-level sink: `f` fills one period of interleaved stereo i16
    /// (`PERIOD_FRAMES` frames) from any renderer (e.g. zpod-voices); false
    /// ends the stream after a short silent drain. MPD is paused and its
    /// output disabled while running, re-enabled (left paused) at shutdown.
    pub fn start_render<F>(mut f: F) -> Result<Self, AudioError>
    where
        F: FnMut(&mut [i16], usize) -> bool + Send + 'static,
    {
        let mpd = MpdExclusive::acquire();
        thread::sleep(Duration::from_millis(80));
        let stop = Arc::new(AtomicBool::new(false));
        let xruns = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let frames_written = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let (stop_t, xruns_t, frames_t) = (Arc::clone(&stop), Arc::clone(&xruns), Arc::clone(&frames_written));
        let alsa_underruns = Arc::new(AtomicU64::new(0));
        let under_t = Arc::clone(&alsa_underruns);
        let join = thread::Builder::new()
            .name("zpod-audio".into())
            .spawn(move || {
                flush_denormals();
                let run = || -> Result<(), AudioError> {
                    let mut out = AplayOut::open(under_t)?;
                    let mut buf = vec![0i16; PERIOD_FRAMES * CHANNELS];
                    let mut errs = 0u32;
                    while !stop_t.load(Ordering::Relaxed) {
                        if !f(&mut buf, PERIOD_FRAMES) {
                            break;
                        }
                        match out.write_i16(&buf) {
                            Ok(()) => {
                                frames_t.fetch_add(PERIOD_FRAMES as u64, Ordering::Relaxed);
                                errs = 0;
                            }
                            Err(e) => {
                                if stop_t.load(Ordering::Relaxed) {
                                    break;
                                }
                                xruns_t.fetch_add(1, Ordering::Relaxed);
                                errs += 1;
                                if errs > 20 {
                                    return Err(e);
                                }
                                thread::sleep(Duration::from_millis(5));
                            }
                        }
                    }
                    buf.iter_mut().for_each(|s| *s = 0);
                    for _ in 0..BUFFER_PERIODS {
                        let _ = out.write_i16(&buf);
                    }
                    out.close();
                    Ok(())
                };
                if let Err(e) = run() {
                    eprintln!("music-core: audio thread end: {e}");
                }
            })
            .map_err(|e| AudioError::Spawn(format!("audio thread: {e}")))?;
        Ok(Self { stop, join: Some(join), mpd: Some(mpd), xruns, frames_written, alsa_underruns })
    }

    /// True while the audio thread is still running.
    pub fn alive(&self) -> bool {
        self.join.as_ref().is_some_and(|j| !j.is_finished())
    }
}

impl Drop for AudioRuntime {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
        if let Some(m) = self.mpd.take() {
            m.release();
        }
    }
}

fn audio_thread(
    perf: Arc<Mutex<PerfState>>,
    stop: Arc<AtomicBool>,
    xruns: Arc<std::sync::atomic::AtomicU64>,
    frames_written: Arc<std::sync::atomic::AtomicU64>,
    underruns: Arc<AtomicU64>,
) -> Result<(), AudioError> {
    let mut out = AplayOut::open(underruns)?;
    let mut eng = Engine::new(DEFAULT_SR);
    let mut buf = vec![0i16; PERIOD_FRAMES * CHANNELS];
    // fix held init
    let mut held: [Option<u8>; 4] = [None; 4];
    let mut samples_into_step: u32 = 0;
    let mut last_chord = Chord4 { notes: [0; 4] };
    let mut silent_periods: u32 = 0;

    while !stop.load(Ordering::Relaxed) {
        let (_chord, pattern, tempo, sounding, fade, exit, x, y, mapper) = {
            let st = perf.lock().unwrap();
            (
                st.chord,
                st.pattern,
                st.tempo_bpm.max(40),
                st.sounding,
                st.fade,
                st.exit,
                st.x,
                st.y,
                st.mapper,
            )
        };
        if exit && fade <= 0.001 {
            break;
        }

        // Recompute chord from mapper each period (UI also writes chord).
        let chord = {
            let c = mapper.chord(x, y);
            if let Ok(mut st) = perf.try_lock() {
                st.chord = c;
            }
            c
        };

        let step_samples = ((SAMPLE_RATE as u64 * 60) / (tempo as u64 * 4)) as u32; // 1/16 note
        samples_into_step += PERIOD_FRAMES as u32;

        if sounding && fade > 0.0 {
            if pattern.is_on() && pattern != Pattern::Chord {
                if samples_into_step >= step_samples {
                    samples_into_step = 0;
                    let step = {
                        let mut st = perf.lock().unwrap();
                        st.arp_step = (st.arp_step + 1) % 4;
                        st.arp_step
                    };
                    let order: [usize; 4] = match pattern {
                        Pattern::Up => [0, 1, 2, 3],
                        Pattern::Down => [3, 2, 1, 0],
                        Pattern::UpDown => [0, 1, 3, 2],
                        _ => [0, 1, 2, 3],
                    };
                    let idx = order[step as usize % 4];
                    // release previous arp note
                    for h in held.iter_mut() {
                        if let Some(m) = h.take() {
                            eng.note_off(m);
                        }
                    }
                    let m = chord.notes[idx];
                    eng.note_on(m, 0.85);
                    held[0] = Some(m);
                }
            } else {
                // Chord mode / pattern off: sustain all four, update on change.
                if chord != last_chord || held.iter().all(|h| h.is_none()) {
                    for h in held.iter_mut() {
                        if let Some(m) = h.take() {
                            eng.note_off(m);
                        }
                    }
                    for (i, &n) in chord.notes.iter().enumerate() {
                        eng.note_on(n, 0.7);
                        held[i] = Some(n);
                    }
                    last_chord = chord;
                }
            }
        } else {
            for h in held.iter_mut() {
                if let Some(m) = h.take() {
                    eng.note_off(m);
                }
            }
        }

        eng.master = fade.clamp(0.0, 1.0);
        eng.render_i16(&mut buf, PERIOD_FRAMES);

        match out.write_i16(&buf) {
            Ok(()) => {
                frames_written.fetch_add(PERIOD_FRAMES as u64, Ordering::Relaxed);
                silent_periods = 0;
            }
            Err(e) => {
                // During shutdown aplay may get SIGPIPE; don't spin.
                if stop.load(Ordering::Relaxed) || exit {
                    break;
                }
                xruns.fetch_add(1, Ordering::Relaxed);
                silent_periods += 1;
                eprintln!("music-core: write err ({silent_periods}): {e}");
                if silent_periods > 20 {
                    return Err(e);
                }
                thread::sleep(Duration::from_millis(5));
            }
        }

        // Apply fade-out steps when exiting.
        if exit {
            if let Ok(mut st) = perf.lock() {
                st.fade = (st.fade - 0.15).max(0.0);
            }
        }
    }

    // Drain silence briefly so aplay's buffer empties quietly.
    for s in buf.iter_mut() {
        *s = 0;
    }
    for _ in 0..BUFFER_PERIODS {
        let _ = out.write_i16(&buf);
    }
    out.close();
    Ok(())
}

/// CPU bench: measure microseconds per voice for `seconds` of audio.
pub fn bench_cpu(seconds: f32, voices: usize) -> BenchResult {
    let mut eng = Engine::new(DEFAULT_SR);
    let n = voices.min(MAX_VOICES);
    for i in 0..n {
        eng.note_on(48 + (i as u8) * 3, 0.8);
    }
    let frames = (DEFAULT_SR * seconds) as usize;
    let mut buf = vec![0.0f32; 64];
    let start = Instant::now();
    let mut done = 0usize;
    while done < frames {
        let n = buf.len().min(frames - done);
        eng.render_f32_mono(&mut buf[..n]);
        done += n;
    }
    let elapsed = start.elapsed();
    let audio_secs = frames as f64 / DEFAULT_SR as f64;
    let cpu_total = elapsed.as_secs_f64() / audio_secs;
    let cpu_per_voice = cpu_total / n.max(1) as f64;
    BenchResult {
        voices: n,
        audio_secs,
        wall_secs: elapsed.as_secs_f64(),
        cpu_frac_total: cpu_total,
        cpu_frac_per_voice: cpu_per_voice,
        recommended_poly: if cpu_per_voice < 0.03 {
            12
        } else if cpu_per_voice < 0.045 {
            8
        } else {
            6
        },
    }
}

#[derive(Clone, Debug)]
pub struct BenchResult {
    pub voices: usize,
    pub audio_secs: f64,
    pub wall_secs: f64,
    pub cpu_frac_total: f64,
    pub cpu_frac_per_voice: f64,
    pub recommended_poly: usize,
}

impl BenchResult {
    pub fn report(&self) -> String {
        format!(
            "bench: {v} voices, {a:.2}s audio in {w:.3}s wall → CPU {t:.1}% total, {p:.2}% per voice; polyphony {poly} (period {per} frames, ~{lat:.1} ms)",
            v = self.voices,
            a = self.audio_secs,
            w = self.wall_secs,
            t = self.cpu_frac_total * 100.0,
            p = self.cpu_frac_per_voice * 100.0,
            poly = self.recommended_poly,
            per = PERIOD_FRAMES,
            lat = (PERIOD_FRAMES * BUFFER_PERIODS) as f64 * 1000.0 / SAMPLE_RATE as f64,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_runs() {
        let r = bench_cpu(0.05, 4);
        assert!(r.wall_secs > 0.0);
        assert!(r.cpu_frac_per_voice > 0.0);
        assert!(r.recommended_poly >= 6);
    }
}

/// Puts the calling thread's FPU in flush-to-zero / default-NaN mode
/// ("RunFast" on ARM VFP). The Pi Zero's ARM1176 handles subnormal floats
/// very slowly; decaying filter, envelope and echo tails reach them and
/// can multiply render cost. Subnormals are below -700 dBFS, so flushing
/// them changes nothing audible. Called at the start of every music-core
/// audio thread; apps rendering on another thread (benchmarks) call it too.
/// No-op on other targets.
pub fn flush_denormals() {
    #[cfg(all(target_arch = "arm", target_abi = "eabihf"))]
    // SAFETY: sets FPSCR bits 24 (FZ) and 25 (DN) for this thread only.
    unsafe {
        let mut r: u32;
        core::arch::asm!("vmrs {0}, fpscr", out(reg) r, options(nomem, nostack));
        r |= (1 << 24) | (1 << 25);
        core::arch::asm!("vmsr fpscr, {0}", in(reg) r, options(nomem, nostack));
    }
}
