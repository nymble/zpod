//! Sentence level: `$<talker><type>,<fields>*<hh>` with an XOR checksum.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NmeaError {
    /// No leading `$` / `!`, or not ASCII, or too long (> 82 chars + slack).
    Framing,
    /// Missing `*hh` (round 1 requires checksums: the ND-105C sends them).
    NoChecksum,
    Checksum { want: u8, got: u8 },
    /// Known sentence with a field that does not parse.
    Field(&'static str),
    /// Valid frame, a sentence type we do not decode (GLL, VTG, ZDA, ...).
    Unsupported,
}

/// Check framing + checksum; returns the payload between `$` and `*`.
pub fn verify(line: &str) -> Result<&str, NmeaError> {
    let line = line.trim_end_matches(['\r', '\n']).trim_start_matches('\0');
    if line.len() > 100 || !line.is_ascii() {
        return Err(NmeaError::Framing);
    }
    let body = line.strip_prefix('$').or_else(|| line.strip_prefix('!')).ok_or(NmeaError::Framing)?;
    let (payload, cs) = body.rsplit_once('*').ok_or(NmeaError::NoChecksum)?;
    if cs.len() != 2 {
        return Err(NmeaError::NoChecksum);
    }
    let want = u8::from_str_radix(cs, 16).map_err(|_| NmeaError::NoChecksum)?;
    let got = payload.bytes().fold(0u8, |a, b| a ^ b);
    if want != got {
        return Err(NmeaError::Checksum { want, got });
    }
    Ok(payload)
}

/// UTC time of day (from hhmmss.ss) and optional date (RMC ddmmyy).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Utc {
    pub h: u8,
    pub m: u8,
    pub s: f32,
}

impl Utc {
    pub fn parse(f: &str) -> Option<Utc> {
        if f.len() < 6 {
            return None;
        }
        let h: u8 = f.get(0..2)?.parse().ok()?;
        let m: u8 = f.get(2..4)?.parse().ok()?;
        let s: f32 = f.get(4..)?.parse().ok()?;
        (h < 24 && m < 60 && s < 61.0).then_some(Utc { h, m, s })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gga {
    pub time: Option<Utc>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// 0 = invalid, 1 = GPS, 2 = DGPS, 4/5 RTK, 6 = estimated ...
    pub quality: u8,
    pub sats_used: Option<u8>,
    pub hdop: Option<f32>,
    /// metres above mean sea level
    pub alt_m: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rmc {
    pub time: Option<Utc>,
    /// 'A' = valid
    pub valid: bool,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub speed_kn: Option<f32>,
    pub course_deg: Option<f32>,
    /// (day, month, year 20yy)
    pub date: Option<(u8, u8, u16)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gsa {
    /// 1 = no fix, 2 = 2D, 3 = 3D
    pub mode: u8,
    pub prns: Vec<u16>,
    pub pdop: Option<f32>,
    pub hdop: Option<f32>,
    pub vdop: Option<f32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GsvSat {
    pub prn: u16,
    pub elev: Option<u8>,
    pub az: Option<u16>,
    /// dB-Hz; None = not tracking
    pub snr: Option<u8>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gsv {
    pub total: u8,
    pub num: u8,
    pub in_view: u8,
    pub sats: Vec<GsvSat>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Sentence {
    Gga(Gga),
    Rmc(Rmc),
    Gsa(Gsa),
    Gsv(Gsv),
}

/// ddmm.mmmm / dddmm.mmmm + hemisphere -> signed decimal degrees.
pub fn coord(v: &str, hemi: &str, deg_digits: usize) -> Result<Option<f64>, NmeaError> {
    if v.is_empty() && hemi.is_empty() {
        return Ok(None);
    }
    let bad = NmeaError::Field("coordinate");
    if v.len() < deg_digits + 2 || !v.is_char_boundary(deg_digits) {
        return Err(bad);
    }
    let d: f64 = v[..deg_digits].parse().map_err(|_| bad)?;
    let m: f64 = v[deg_digits..].parse().map_err(|_| bad)?;
    if m >= 60.0 {
        return Err(bad);
    }
    let x = d + m / 60.0;
    let x = match hemi {
        "N" | "E" => x,
        "S" | "W" => -x,
        _ => return Err(bad),
    };
    let lim = if deg_digits == 2 { 90.0 } else { 180.0 };
    if x.abs() > lim {
        return Err(bad);
    }
    Ok(Some(x))
}

fn opt<T: std::str::FromStr>(f: &str, what: &'static str) -> Result<Option<T>, NmeaError> {
    if f.is_empty() {
        Ok(None)
    } else {
        f.parse().map(Some).map_err(|_| NmeaError::Field(what))
    }
}

/// Parse a full line (checksum verified first).
pub fn parse_sentence(line: &str) -> Result<Sentence, NmeaError> {
    let payload = verify(line)?;
    let f: Vec<&str> = payload.split(',').collect();
    let id = f[0];
    if id.len() != 5 {
        return Err(NmeaError::Unsupported);
    }
    let g = |i: usize| f.get(i).copied().unwrap_or("");
    match &id[2..] {
        "GGA" => Ok(Sentence::Gga(Gga {
            time: Utc::parse(g(1)),
            lat: coord(g(2), g(3), 2)?,
            lon: coord(g(4), g(5), 3)?,
            quality: opt::<u8>(g(6), "quality")?.unwrap_or(0),
            sats_used: opt(g(7), "satellites")?,
            hdop: opt(g(8), "hdop")?,
            alt_m: opt(g(9), "altitude")?,
        })),
        "RMC" => {
            let date = if g(9).len() == 6 {
                let p = |a: usize| g(9)[a..a + 2].parse::<u8>().ok();
                match (p(0), p(2), p(4)) {
                    (Some(d), Some(m), Some(y)) if (1..=31).contains(&d) && (1..=12).contains(&m) => Some((d, m, 2000 + y as u16)),
                    _ => return Err(NmeaError::Field("date")),
                }
            } else {
                None
            };
            Ok(Sentence::Rmc(Rmc {
                time: Utc::parse(g(1)),
                valid: g(2) == "A",
                lat: coord(g(3), g(4), 2)?,
                lon: coord(g(5), g(6), 3)?,
                speed_kn: opt(g(7), "speed")?,
                course_deg: opt(g(8), "course")?,
                date,
            }))
        }
        "GSA" => {
            let mode = opt::<u8>(g(2), "fix mode")?.unwrap_or(1);
            let mut prns = vec![];
            for i in 3..15 {
                if let Some(p) = opt::<u16>(g(i), "prn")? {
                    prns.push(p);
                }
            }
            // NMEA 4.1 adds a system id after VDOP: strip anything after it
            Ok(Sentence::Gsa(Gsa { mode, prns, pdop: opt(g(15), "pdop")?, hdop: opt(g(16), "hdop")?, vdop: opt(g(17), "vdop")? }))
        }
        "GSV" => {
            let total = opt::<u8>(g(1), "gsv total")?.ok_or(NmeaError::Field("gsv total"))?;
            let num = opt::<u8>(g(2), "gsv number")?.ok_or(NmeaError::Field("gsv number"))?;
            let in_view = opt::<u8>(g(3), "in view")?.unwrap_or(0);
            if num == 0 || num > total {
                return Err(NmeaError::Field("gsv number"));
            }
            let mut sats = vec![];
            // groups of 4 (prn, elevation, azimuth, snr); NMEA 4.1 adds one
            // lone signal-id field at the end, which the integer division drops
            for k in 0..f.len().saturating_sub(4) / 4 {
                let i = 4 + 4 * k;
                let Some(prn) = opt::<u16>(g(i), "prn")? else { continue };
                sats.push(GsvSat { prn, elev: opt(g(i + 1), "elevation")?, az: opt(g(i + 2), "azimuth")?, snr: opt(g(i + 3), "snr")? });
            }
            Ok(Sentence::Gsv(Gsv { total, num, in_view, sats }))
        }
        _ => Err(NmeaError::Unsupported),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a line with a correct checksum.
    pub fn line(payload: &str) -> String {
        let cs = payload.bytes().fold(0u8, |a, b| a ^ b);
        format!("${payload}*{cs:02X}\r\n")
    }

    #[test]
    fn checksum_and_framing() {
        // classic example sentence
        let l = "$GPGGA,123519,4807.038,N,01131.000,E,1,08,0.9,545.4,M,46.9,M,,*47";
        assert!(verify(l).is_ok());
        assert_eq!(verify("$GPGGA,123519,4807.038,N,01131.000,E,1,08,0.9,545.4,M,46.9,M,,*48"), Err(NmeaError::Checksum { want: 0x48, got: 0x47 }));
        assert_eq!(verify("GPGGA,1*00"), Err(NmeaError::Framing));
        assert_eq!(verify("$GPGGA,123519"), Err(NmeaError::NoChecksum));
        assert_eq!(verify("$GPGGA,1*4"), Err(NmeaError::NoChecksum));
        assert_eq!(verify("$GP\u{e9}GA*00"), Err(NmeaError::Framing));
        assert!(verify(&line("GPRMC,,V,,,,,,,,,,N")).is_ok());
    }

    #[test]
    fn gga() {
        let Sentence::Gga(g) = parse_sentence("$GPGGA,123519,4807.038,N,01131.000,E,1,08,0.9,545.4,M,46.9,M,,*47").unwrap() else { panic!() };
        assert_eq!(g.time, Some(Utc { h: 12, m: 35, s: 19.0 }));
        assert!((g.lat.unwrap() - 48.1173).abs() < 1e-4);
        assert!((g.lon.unwrap() - 11.516_666).abs() < 1e-4);
        assert_eq!((g.quality, g.sats_used, g.hdop, g.alt_m), (1, Some(8), Some(0.9), Some(545.4)));
        // no fix: empty fields
        let Sentence::Gga(g) = parse_sentence(&line("GPGGA,000012.000,,,,,0,00,,,M,,M,,")).unwrap() else { panic!() };
        assert_eq!((g.lat, g.lon, g.quality, g.sats_used), (None, None, 0, Some(0)));
    }

    #[test]
    fn rmc_south_west() {
        let Sentence::Rmc(r) = parse_sentence(&line("GPRMC,225446.00,A,3345.1234,S,11822.5000,W,0.5,54.7,191194,020.3,E")).unwrap() else { panic!() };
        assert!(r.valid);
        assert!((r.lat.unwrap() + 33.752_056).abs() < 1e-5);
        assert!((r.lon.unwrap() + 118.375).abs() < 1e-6);
        assert_eq!(r.date, Some((19, 11, 2094)));
        assert_eq!(r.speed_kn, Some(0.5));
        assert!(parse_sentence(&line("GPRMC,225446,A,9345.0,N,00000.0,E,,,191194,,")).is_err(), "lat > 90");
    }

    #[test]
    fn gsa_and_gsv() {
        let Sentence::Gsa(a) = parse_sentence(&line("GPGSA,A,3,04,05,,09,12,,,24,,,,,2.5,1.3,2.1")).unwrap() else { panic!() };
        assert_eq!((a.mode, a.prns.clone(), a.hdop), (3, vec![4, 5, 9, 12, 24], Some(1.3)));
        let Sentence::Gsv(v) = parse_sentence(&line("GPGSV,3,1,11,03,03,111,00,04,15,270,00,06,01,010,00,13,06,292,")).unwrap() else { panic!() };
        assert_eq!((v.total, v.num, v.in_view, v.sats.len()), (3, 1, 11, 4));
        assert_eq!(v.sats[3], GsvSat { prn: 13, elev: Some(6), az: Some(292), snr: None });
        let Sentence::Gsv(v) = parse_sentence(&line("GPGSV,3,3,11,22,42,067,42,24,14,311,43,27,05,244,00")).unwrap() else { panic!() };
        assert_eq!(v.sats.len(), 3);
        assert_eq!(v.sats[1].snr, Some(43));
        assert_eq!(parse_sentence(&line("GPGSV,2,3,08")), Err(NmeaError::Field("gsv number")));
    }

    #[test]
    fn unsupported_but_valid() {
        assert_eq!(parse_sentence(&line("GPVTG,054.7,T,034.4,M,005.5,N,010.2,K")), Err(NmeaError::Unsupported));
    }
}
