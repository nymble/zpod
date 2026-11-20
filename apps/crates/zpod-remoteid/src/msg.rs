//! The 25-byte Remote ID messages.

pub const MSG_LEN: usize = 25;

#[derive(Clone, Debug, PartialEq)]
pub struct BasicId {
    /// 0 none, 1 serial (CTA-2063-A), 2 CAA registration, 3 UTM UUID, 4 session id
    pub id_type: u8,
    pub ua_type: u8,
    pub uas_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    /// 0 undeclared, 1 ground, 2 airborne, 3 emergency, 4 RID system failure
    pub status: u8,
    /// degrees, None = unknown (raw 361)
    pub track_deg: Option<u16>,
    /// m/s, None = unknown (raw 255)
    pub speed_h: Option<f32>,
    /// m/s, None = unknown (raw 63 m/s)
    pub speed_v: Option<f32>,
    /// None when both raw lat/lon are 0 (no position)
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// metres, None = unknown (raw 0 = -1000 m)
    pub alt_baro: Option<f32>,
    pub alt_geo: Option<f32>,
    pub height: Option<f32>,
    /// true: height above ground; false: above takeoff
    pub height_agl: bool,
    pub h_acc: u8,
    pub v_acc: u8,
    /// 1/10 s since the full hour, None = unknown (0xFFFF)
    pub tenths_after_hour: Option<u16>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelfId {
    pub desc_type: u8,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct System {
    /// 0 takeoff, 1 live GNSS, 2 fixed
    pub operator_loc_type: u8,
    pub classification: u8,
    pub op_lat: Option<f64>,
    pub op_lon: Option<f64>,
    pub area_count: u16,
    pub area_radius_m: u16,
    pub area_ceiling: Option<f32>,
    pub area_floor: Option<f32>,
    pub ua_category: u8,
    pub ua_class: u8,
    pub op_alt_geo: Option<f32>,
    /// seconds since 2019-01-01 00:00 UTC
    pub timestamp: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OperatorId {
    pub id_type: u8,
    pub id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    BasicId(BasicId),
    Location(Location),
    /// authentication page: (auth type, page number); payload kept as hex in captures
    Auth { auth_type: u8, page: u8 },
    SelfId(SelfId),
    System(System),
    OperatorId(OperatorId),
    /// message type nibble not known (protocol extensions)
    Unknown(u8),
}

impl Message {
    pub fn kind(&self) -> &'static str {
        match self {
            Message::BasicId(_) => "Basic ID",
            Message::Location(_) => "Location",
            Message::Auth { .. } => "Auth",
            Message::SelfId(_) => "Self-ID",
            Message::System(_) => "System",
            Message::OperatorId(_) => "Operator ID",
            Message::Unknown(_) => "Unknown",
        }
    }
}

fn text(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    b[..end].iter().map(|&c| if (0x20..0x7f).contains(&c) { c as char } else { '.' }).collect::<String>().trim_end().to_string()
}
fn le16(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn le32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}
fn coord(b: &[u8], i: usize) -> i32 {
    le32(b, i) as i32
}
/// Altitudes: raw*0.5 - 1000 m; raw 0 = unknown.
fn alt(raw: u16) -> Option<f32> {
    (raw != 0).then(|| raw as f32 * 0.5 - 1000.0)
}
fn latlon(lat: i32, lon: i32) -> (Option<f64>, Option<f64>) {
    if lat == 0 && lon == 0 {
        return (None, None);
    }
    let (la, lo) = (lat as f64 * 1e-7, lon as f64 * 1e-7);
    if !(-90.0..=90.0).contains(&la) || !(-180.0..=180.0).contains(&lo) {
        return (None, None);
    }
    (Some(la), Some(lo))
}

pub fn ua_type_label(t: u8) -> &'static str {
    match t {
        0 => "none",
        1 => "aeroplane",
        2 => "multirotor",
        3 => "gyroplane",
        4 => "hybrid VTOL",
        5 => "ornithopter",
        6 => "glider",
        7 => "kite",
        8 => "free balloon",
        9 => "captive balloon",
        10 => "airship",
        11 => "parachute",
        12 => "rocket",
        13 => "tethered",
        14 => "ground obstacle",
        _ => "other",
    }
}
pub fn id_type_label(t: u8) -> &'static str {
    match t {
        1 => "serial",
        2 => "CAA reg",
        3 => "UTM UUID",
        4 => "session",
        _ => "none",
    }
}
pub fn status_label(s: u8) -> &'static str {
    match s {
        1 => "ground",
        2 => "airborne",
        3 => "EMERGENCY",
        4 => "RID failure",
        _ => "undeclared",
    }
}

/// Decode one 25-byte message. `None` if shorter than 25 bytes.
pub fn decode(b: &[u8]) -> Option<Message> {
    if b.len() < MSG_LEN {
        return None;
    }
    let ty = b[0] >> 4;
    Some(match ty {
        0 => Message::BasicId(BasicId { id_type: b[1] >> 4, ua_type: b[1] & 0x0f, uas_id: text(&b[2..22]) }),
        1 => {
            let flags = b[1];
            let ew = flags & 0x02 != 0;
            let mult = flags & 0x01 != 0;
            let dir_raw = b[2] as u16 + if ew { 180 } else { 0 };
            let sp = b[3];
            let speed_h = if sp == 255 { None } else if mult { Some(sp as f32 * 0.75 + 255.0 * 0.25) } else { Some(sp as f32 * 0.25) };
            let vs = b[4] as i8;
            let (lat, lon) = latlon(coord(b, 5), coord(b, 9));
            let ts = le16(b, 21);
            Message::Location(Location {
                status: flags >> 4,
                track_deg: (dir_raw <= 360).then_some(dir_raw % 360),
                speed_h,
                speed_v: if vs == 126 { None } else { Some(vs as f32 * 0.5) },
                lat,
                lon,
                alt_baro: alt(le16(b, 13)),
                alt_geo: alt(le16(b, 15)),
                height: alt(le16(b, 17)),
                height_agl: flags & 0x04 != 0,
                v_acc: b[19] >> 4,
                h_acc: b[19] & 0x0f,
                tenths_after_hour: (ts != 0xffff && ts <= 36000).then_some(ts),
            })
        }
        2 => Message::Auth { auth_type: b[1] >> 4, page: b[1] & 0x0f },
        3 => Message::SelfId(SelfId { desc_type: b[1], text: text(&b[2..25]) }),
        4 => {
            let (op_lat, op_lon) = latlon(coord(b, 2), coord(b, 6));
            Message::System(System {
                operator_loc_type: b[1] & 0x03,
                classification: (b[1] >> 2) & 0x07,
                op_lat,
                op_lon,
                area_count: le16(b, 10),
                area_radius_m: b[12] as u16 * 10,
                area_ceiling: alt(le16(b, 13)),
                area_floor: alt(le16(b, 15)),
                ua_category: b[17] >> 4,
                ua_class: b[17] & 0x0f,
                op_alt_geo: alt(le16(b, 18)),
                timestamp: le32(b, 20),
            })
        }
        5 => Message::OperatorId(OperatorId { id_type: b[1], id: text(&b[2..22]) }),
        t => Message::Unknown(t),
    })
}

/// Decode a message pack (header type 0xF) or a single message.
/// Returns at most 9 messages; malformed packs yield what fits.
pub fn decode_pack(b: &[u8]) -> Vec<Message> {
    let mut v = Vec::new();
    if b.len() < 3 {
        return v;
    }
    if b[0] >> 4 != 0xF {
        if let Some(m) = decode(b) {
            v.push(m);
        }
        return v;
    }
    let size = b[1] as usize;
    let n = (b[2] as usize).min(9);
    if size != MSG_LEN {
        return v;
    }
    for i in 0..n {
        let s = 3 + i * MSG_LEN;
        let Some(chunk) = b.get(s..s + MSG_LEN) else { break };
        if chunk[0] >> 4 == 0xF {
            continue; // no nested packs
        }
        if let Some(m) = decode(chunk) {
            v.push(m);
        }
    }
    v
}
