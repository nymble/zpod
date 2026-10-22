//! High-score table (top 5), persisted through [`zpod_core::Storage`] as a
//! tiny text file: one "score lines level" line per entry.

use zpod_core::Storage;

pub const KEY: &str = "highscores.txt";
pub const N: usize = 5;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub score: u32,
    pub lines: u32,
    pub level: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HighScores {
    pub e: [Entry; N],
}

impl HighScores {
    pub fn best(&self) -> u32 {
        self.e[0].score
    }

    pub fn parse(text: &str) -> Self {
        let mut h = HighScores::default();
        let mut i = 0;
        for line in text.lines() {
            let mut it = line.split_whitespace().map(|x| x.parse::<u32>().unwrap_or(0));
            let (Some(score), lines, level) = (it.next(), it.next().unwrap_or(0), it.next().unwrap_or(1)) else { continue };
            if i < N && score > 0 {
                h.e[i] = Entry { score, lines, level };
                i += 1;
            }
        }
        h.e.sort_by(|a, b| b.score.cmp(&a.score));
        h
    }

    /// Serialise into `buf`, returns length.
    pub fn write(&self, buf: &mut [u8]) -> usize {
        let mut n = 0;
        for e in self.e.iter().filter(|e| e.score > 0) {
            for (k, v) in [e.score, e.lines, e.level].iter().enumerate() {
                n += write_u32(&mut buf[n..], *v);
                buf[n] = if k == 2 { b'\n' } else { b' ' };
                n += 1;
            }
        }
        n
    }

    /// Insert if it qualifies; returns the rank (0-based).
    pub fn submit(&mut self, e: Entry) -> Option<usize> {
        if e.score == 0 {
            return None;
        }
        let pos = self.e.iter().position(|x| e.score > x.score)?;
        self.e.copy_within(pos..N - 1, pos + 1);
        self.e[pos] = e;
        Some(pos)
    }

    pub fn load(st: &mut impl Storage) -> Self {
        let mut buf = [0u8; 256];
        match st.load(KEY, &mut buf) {
            Some(n) => Self::parse(core::str::from_utf8(&buf[..n]).unwrap_or("")),
            None => Self::default(),
        }
    }

    pub fn save(&self, st: &mut impl Storage) -> bool {
        let mut buf = [0u8; 256];
        let n = self.write(&mut buf);
        st.save(KEY, &buf[..n])
    }
}

fn write_u32(buf: &mut [u8], mut v: u32) -> usize {
    let mut tmp = [0u8; 10];
    let mut i = 0;
    loop {
        tmp[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i += 1;
        if v == 0 {
            break;
        }
    }
    for k in 0..i {
        buf[k] = tmp[i - 1 - k];
    }
    i
}
