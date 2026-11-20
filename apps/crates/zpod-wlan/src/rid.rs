//! Remote ID out of a captured radiotap frame.

use crate::frame::{self, mac_str, Kind};
use crate::radiotap;
use zpod_remoteid::{from_nan_service_info, from_wifi_beacon_ie, Message, Transport, NAN_SERVICE_ID};

#[derive(Clone, Debug, PartialEq)]
pub struct Decoded {
    pub src: String,
    pub transport: Transport,
    pub counter: u8,
    pub msgs: Vec<Message>,
    pub rssi: Option<i8>,
    pub freq: Option<u16>,
    /// beacon SSID if any
    pub ssid: Option<String>,
}

/// Per-capture counters for the status line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub frames: u64,
    pub beacons: u64,
    pub probes: u64,
    pub action: u64,
    pub data: u64,
    pub bad_fcs: u64,
    pub rid: u64,
    pub unparsed: u64,
}

/// `pkt` = radiotap header + 802.11 frame (as from a monitor interface).
pub fn decode_frame(pkt: &[u8], st: &mut Stats) -> Option<Decoded> {
    st.frames += 1;
    let Some(rt) = radiotap::parse(pkt) else {
        st.unparsed += 1;
        return None;
    };
    if rt.bad_fcs() {
        st.bad_fcs += 1;
        return None;
    }
    let mut f = &pkt[rt.len..];
    if rt.has_fcs() && f.len() >= 4 {
        f = &f[..f.len() - 4];
    }
    let Some(h) = frame::header(f) else {
        st.unparsed += 1;
        return None;
    };
    let found = match h.kind {
        Kind::Beacon | Kind::ProbeResp => {
            st.beacons += 1;
            let ies = frame::beacon_ies(f);
            let ssid = frame::ssid(&ies);
            ies.iter()
                .filter(|(id, _)| *id == 221)
                .find_map(|(_, b)| from_wifi_beacon_ie(b))
                .map(|(c, m)| Decoded { src: mac_str(&h.sa), transport: Transport::WifiBeacon, counter: c, msgs: m, rssi: rt.signal, freq: rt.freq, ssid })
        }
        Kind::Action => {
            st.action += 1;
            frame::nan_sdas(f)
                .into_iter()
                .filter(|(sid, _)| *sid == NAN_SERVICE_ID)
                .find_map(|(_, si)| from_nan_service_info(si))
                .map(|(c, m)| Decoded { src: mac_str(&h.sa), transport: Transport::WifiNan, counter: c, msgs: m, rssi: rt.signal, freq: rt.freq, ssid: None })
        }
        Kind::ProbeReq => {
            st.probes += 1;
            None
        }
        Kind::Data => {
            st.data += 1;
            None
        }
        _ => None,
    };
    if found.is_some() {
        st.rid += 1;
    }
    found
}
