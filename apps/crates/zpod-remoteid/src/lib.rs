//! Receive-only Remote ID (ASTM F3411-22a / ASD-STAN prEN 4709-002) decoder.
//!
//! Transports handled (decode only; this crate cannot build or send frames):
//! * Bluetooth LE: AD type 0x16 Service Data, UUID 0xFFFA, app code 0x0D,
//!   counter, one 25-byte message (BT4 legacy) or a message pack (BT5).
//! * Wi-Fi Beacon: vendor-specific IE (221), OUI FA:0B:BC, type 0x0D,
//!   counter, message pack.
//! * Wi-Fi NAN: Service Discovery Frame Service Descriptor Attribute whose
//!   service ID is the 6-byte hash of "org.opendroneid.remoteid"; its
//!   service info = counter + message pack.
//!
//! Every decoder takes a byte slice and returns `None`/`Err` on short or
//! malformed input; nothing panics on hostile bytes (see tests).

pub mod display;
pub mod msg;
pub mod transport;
pub mod track;
#[doc(hidden)]
pub mod sample;

pub use msg::{decode, decode_pack, BasicId, Location, Message, OperatorId, SelfId, System, MSG_LEN};
pub use transport::{from_ble_service_data, from_nan_service_info, from_wifi_beacon_ie, Transport, NAN_SERVICE_ID, RID_BLE_UUID, RID_OUI};
pub use track::{Aircraft, Tracker};

#[cfg(test)]
mod tests;
