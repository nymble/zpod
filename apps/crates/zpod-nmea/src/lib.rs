//! NMEA 0183 for the ZPOD GPS tool (GlobalSat ND-105C and other serial GNSS
//! receivers): checksummed sentence parsing (GGA, RMC, GSA, GSV) and a
//! receiver state machine with the UI states of docs/USB-HOST-PLAN.md §8.4.
//! Pure logic, no I/O: the tool owns the serial port and the clock.

pub mod parse;
pub mod state;

pub use parse::{parse_sentence, verify, Gga, Gsa, Gsv, GsvSat, NmeaError, Rmc, Sentence, Utc};
pub use state::{FixKind, GpsState, GpsStatus, Sat, STALE_MS};
