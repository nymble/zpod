//! zpod-midi: send an app's note events to a MIDI port.
//!
//! Not tied to any app, UI or audio backend. An app converts its own events
//! to [`router::In`] and calls [`router::Router::block`] once per audio block
//! on its render thread (no allocation, no locks, no IO there). Messages go
//! through a fixed-size lock-free queue to a low-priority worker thread
//! ([`out::MidiOut`]) that writes them to a [`sink::MidiSink`] at their due
//! time: ALSA rawmidi (`/dev/snd/midiC*D*`, non-blocking) on the device, or a
//! fake sink in tests.
//!
//! Safety cleanup: every routing change, transport stop, queue overflow,
//! write failure, reconnect and shutdown ends sounding notes (note-off for
//! each note actually sent, then CC 123 All Notes Off per used channel).

pub mod config;
pub mod encode;
pub mod out;
pub mod queue;
pub mod router;
pub mod sink;

pub use config::{Fallback, Lane, MidiConfig, PortName, Route, LANES};
pub use encode::Msg;
pub use out::{MidiOut, State, Status};
pub use router::{In, InEvent, Router};
pub use sink::{AlsaOpener, FakeOpener, FakeSink, MidiSink, Opener, PortInfo, SinkError};

#[cfg(test)]
mod tests;
