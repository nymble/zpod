//! Routing configuration and its strict TOML file (`zpod-midi` v1).
//!
//! The default is always `route = internal`: MIDI never turns on by itself,
//! and a missing or invalid file means Internal.

use serde::{Deserialize, Serialize};

/// Most lanes an app can map.
pub const LANES: usize = 8;
pub const PORT_MAX: usize = 32;
pub const BEND_RANGE_MAX: u8 = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Internal,
    Midi,
    Both,
}

impl Route {
    pub const ALL: [Route; 3] = [Route::Internal, Route::Midi, Route::Both];
    pub fn key(self) -> &'static str {
        match self {
            Route::Internal => "internal",
            Route::Midi => "midi",
            Route::Both => "both",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Route::Internal => "Internal",
            Route::Midi => "MIDI",
            Route::Both => "Both",
        }
    }
    pub fn sends(self) -> bool {
        self != Route::Internal
    }
}

/// What to do when route = MIDI but no port is connected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallback {
    /// play the internal sound until a port is connected (default)
    Internal,
    /// stay silent (the status shows it)
    Silent,
}

impl Fallback {
    pub fn key(self) -> &'static str {
        match self {
            Fallback::Internal => "internal",
            Fallback::Silent => "silent",
        }
    }
}

/// Per-lane mapping: channel 1..=16; `note` = fixed note (drum map) or None
/// to play the lane's own pitch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lane {
    pub channel: u8,
    pub note: Option<u8>,
}

/// A short port name without allocation ("" = auto: first port found).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PortName {
    buf: [u8; PORT_MAX],
    len: u8,
}

impl PortName {
    pub const AUTO: PortName = PortName { buf: [0; PORT_MAX], len: 0 };
    pub fn new(s: &str) -> Result<PortName, String> {
        if s.eq_ignore_ascii_case("auto") || s.is_empty() {
            return Ok(Self::AUTO);
        }
        if s.len() > PORT_MAX || !s.bytes().all(|b| (0x20..0x7f).contains(&b)) {
            return Err(format!("port '{s}': up to {PORT_MAX} printable ASCII chars"));
        }
        let mut buf = [0; PORT_MAX];
        buf[..s.len()].copy_from_slice(s.as_bytes());
        Ok(PortName { buf, len: s.len() as u8 })
    }
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len as usize]).unwrap_or("")
    }
    pub fn is_auto(&self) -> bool {
        self.len == 0
    }
}

impl core::fmt::Debug for PortName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", if self.is_auto() { "auto" } else { self.as_str() })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MidiConfig {
    pub route: Route,
    pub fallback: Fallback,
    pub port: PortName,
    /// send MIDI Clock (24 per quarter note) and Start/Stop
    pub clock: bool,
    /// receiver's pitch-bend range in semitones (1..=24), used for cents
    pub bend_range: u8,
    pub lanes: [Lane; LANES],
    pub nlanes: u8,
}

impl MidiConfig {
    /// Internal routing with the given lane map.
    pub fn new(lanes: &[Lane]) -> MidiConfig {
        let mut l = [Lane { channel: 1, note: None }; LANES];
        let n = lanes.len().min(LANES);
        l[..n].copy_from_slice(&lanes[..n]);
        MidiConfig { route: Route::Internal, fallback: Fallback::Internal, port: PortName::AUTO, clock: false, bend_range: 2, lanes: l, nlanes: n as u8 }
    }
    pub fn lanes(&self) -> &[Lane] {
        &self.lanes[..self.nlanes as usize]
    }
    /// Channels used by any lane, as a 16-bit mask (bit 0 = channel 1).
    pub fn channel_mask(&self) -> u16 {
        self.lanes().iter().fold(0, |m, l| m | 1 << (l.channel.clamp(1, 16) - 1))
    }
    /// Whether the app should render its internal sound now.
    pub fn internal_audible(&self, connected: bool) -> bool {
        match self.route {
            Route::Internal | Route::Both => true,
            Route::Midi => !connected && self.fallback == Fallback::Internal,
        }
    }
    /// Same routing for sounding notes (a change needs a panic first).
    pub fn same_routing(&self, o: &MidiConfig) -> bool {
        self.route == o.route && self.port == o.port && self.lanes() == o.lanes() && self.bend_range == o.bend_range
    }

    /// Strict parse: unknown keys, wrong lengths and out-of-range values are
    /// errors. `defaults` supplies the lane count.
    pub fn from_toml(s: &str, defaults: &MidiConfig) -> Result<MidiConfig, String> {
        let f: File = toml::from_str(s).map_err(|e| e.message().to_string())?;
        if f.format != "zpod-midi" {
            return Err(format!("format '{}' is not zpod-midi", f.format));
        }
        if f.version != 1 {
            return Err(format!("version {} not supported (1)", f.version));
        }
        let mut c = *defaults;
        c.route = match f.route.as_str() {
            "internal" => Route::Internal,
            "midi" => Route::Midi,
            "both" => Route::Both,
            r => return Err(format!("route '{r}': internal, midi or both")),
        };
        c.fallback = match f.fallback.as_str() {
            "internal" => Fallback::Internal,
            "silent" => Fallback::Silent,
            r => return Err(format!("fallback '{r}': internal or silent")),
        };
        c.port = PortName::new(&f.port)?;
        c.clock = f.clock;
        if !(1..=BEND_RANGE_MAX).contains(&f.bend_range) {
            return Err(format!("bend_range {}: 1..{BEND_RANGE_MAX}", f.bend_range));
        }
        c.bend_range = f.bend_range;
        let n = defaults.nlanes as usize;
        if f.channels.len() != n || f.notes.len() != n {
            return Err(format!("channels and notes need {n} values each"));
        }
        for i in 0..n {
            let ch = f.channels[i];
            if !(1..=16).contains(&ch) {
                return Err(format!("channels[{i}] = {ch}: 1..16"));
            }
            let note = match f.notes[i] {
                -1 => None,
                v @ 0..=127 => Some(v as u8),
                v => return Err(format!("notes[{i}] = {v}: 0..127, or -1 for the lane's pitch")),
            };
            c.lanes[i] = Lane { channel: ch as u8, note };
        }
        Ok(c)
    }

    pub fn to_toml(&self) -> String {
        let f = File {
            format: "zpod-midi".into(),
            version: 1,
            route: self.route.key().into(),
            fallback: self.fallback.key().into(),
            port: if self.port.is_auto() { "auto".into() } else { self.port.as_str().into() },
            clock: self.clock,
            bend_range: self.bend_range,
            channels: self.lanes().iter().map(|l| l.channel as i64).collect(),
            notes: self.lanes().iter().map(|l| l.note.map(|n| n as i64).unwrap_or(-1)).collect(),
        };
        toml::to_string(&f).unwrap_or_default()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    format: String,
    version: u32,
    route: String,
    fallback: String,
    port: String,
    clock: bool,
    bend_range: u8,
    channels: Vec<i64>,
    notes: Vec<i64>,
}
