//! Moving TEST aircraft for track-to-map testing (encode only, no RF).
//!
//! Models position, ground speed, track (direction), altitude and climb rate
//! over time, with an optional constant turn rate, so a receiver or map
//! under test sees a smoothly moving aircraft. Flat-earth step (fine for
//! the few-km tracks a lab test uses).

use crate::encode::{self, TEST_SELF_ID, TEST_UAS_ID};
use crate::msg::{BasicId, Location, OperatorId, SelfId, System, MSG_LEN};

const M_PER_DEG_LAT: f64 = 111_320.0;

#[derive(Clone, Debug, PartialEq)]
pub struct SimAircraft {
    pub uas_id: String,
    pub lat: f64,
    pub lon: f64,
    /// geodetic altitude, m
    pub alt_m: f32,
    /// height above takeoff, m
    pub height_m: f32,
    /// ground speed, m/s
    pub speed_mps: f32,
    /// direction of travel, degrees from true north (0..360)
    pub track_deg: f32,
    /// vertical speed, m/s (positive = climbing)
    pub climb_mps: f32,
    /// degrees per second, positive = clockwise
    pub turn_dps: f32,
    /// operator (takeoff) position
    pub op_lat: f64,
    pub op_lon: f64,
    /// seconds since start
    pub t_s: f64,
}

impl SimAircraft {
    /// A labelled test aircraft at `lat`,`lon`, 50 m up, 10 m/s heading east.
    pub fn test(lat: f64, lon: f64) -> SimAircraft {
        SimAircraft {
            uas_id: TEST_UAS_ID.into(),
            lat,
            lon,
            alt_m: 50.0,
            height_m: 50.0,
            speed_mps: 10.0,
            track_deg: 90.0,
            climb_mps: 0.0,
            turn_dps: 0.0,
            op_lat: lat,
            op_lon: lon,
            t_s: 0.0,
        }
    }

    /// Advance the model by `dt` seconds.
    pub fn step(&mut self, dt: f64) {
        let tr = (self.track_deg as f64).to_radians();
        let d = self.speed_mps as f64 * dt;
        self.lat += d * tr.cos() / M_PER_DEG_LAT;
        self.lon += d * tr.sin() / (M_PER_DEG_LAT * self.lat.to_radians().cos().max(1e-6));
        let dz = self.climb_mps * dt as f32;
        self.alt_m += dz;
        self.height_m = (self.height_m + dz).max(0.0);
        self.track_deg = (self.track_deg + self.turn_dps * dt as f32).rem_euclid(360.0);
        self.t_s += dt;
    }

    pub fn location(&self, tenths_after_hour: u16) -> Location {
        Location {
            status: if self.height_m > 0.5 { 2 } else { 1 },
            track_deg: Some(self.track_deg.round() as u16 % 360),
            speed_h: Some(self.speed_mps),
            speed_v: Some(self.climb_mps),
            lat: Some(self.lat),
            lon: Some(self.lon),
            alt_baro: Some(self.alt_m),
            alt_geo: Some(self.alt_m),
            height: Some(self.height_m),
            height_agl: false,
            h_acc: 10,
            v_acc: 4,
            tenths_after_hour: Some(tenths_after_hour % 36000),
        }
    }

    /// Basic ID + Location + Self ID ("ZPOD TEST") + System + Operator ID.
    pub fn messages(&self, tenths_after_hour: u16, timestamp: u32) -> Vec<[u8; MSG_LEN]> {
        vec![
            encode::basic_id(&BasicId { id_type: 1, ua_type: 2, uas_id: self.uas_id.clone() }),
            encode::location(&self.location(tenths_after_hour)),
            encode::self_id(&SelfId { desc_type: 0, text: TEST_SELF_ID.into() }),
            encode::system(&System {
                operator_loc_type: 0,
                classification: 0,
                op_lat: Some(self.op_lat),
                op_lon: Some(self.op_lon),
                area_count: 1,
                area_radius_m: 0,
                area_ceiling: None,
                area_floor: None,
                ua_category: 0,
                ua_class: 0,
                op_alt_geo: None,
                timestamp,
            }),
            encode::operator_id(&OperatorId { id_type: 0, id: "ZPODTESTOPERATOR".into() }),
        ]
    }
}
