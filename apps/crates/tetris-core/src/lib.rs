//! Tetris engine, written from scratch for ZPOD, portable to any platform.
//!
//! * `no_std`, no allocator needed for the game logic ([`Game`]).
//! * Guideline behaviour: 10x20 field (+2 hidden rows), 7 tetrominoes,
//!   7-bag randomiser, SRS rotation with the standard JLSTZ and I wall-kick
//!   tables, hold (once per piece), 5-piece next queue, ghost piece, lock
//!   delay (500 ms, 15 move resets), DAS/ARR handled in the engine,
//!   guideline gravity curve, level up every 10 lines, T-spin (3-corner)
//!   detection, back-to-back and combo scoring, line clear animation phase.
//! * Time is passed in as milliseconds ([`Game::tick`]); input as
//!   [`Action`]s, so the frontend decides how buttons map.
//! * The optional `render` feature draws the game on any
//!   `embedded-graphics` `DrawTarget<Color = Rgb565>`.
#![no_std]

#[cfg(feature = "render")]
extern crate alloc;
#[cfg(feature = "render")]
pub mod render;
pub mod scores;

pub const COLS: usize = 10;
pub const HIDDEN: usize = 2;
pub const ROWS: usize = 20 + HIDDEN;

pub const CLEAR_ANIM_MS: u32 = 320;
pub const LOCK_DELAY_MS: u32 = 500;
pub const MAX_LOCK_RESETS: u8 = 15;
pub const DAS_MS: u32 = 170;
pub const ARR_MS: u32 = 50;
/// Soft drop speed: rows per second.
pub const SOFT_DROP_MS: u32 = 33;
pub const MAX_LEVEL: u32 = 20;

/// Guideline gravity: ms per row = (0.8 - (level-1)*0.007)^(level-1) s.
const GRAVITY_MS: [u32; 20] = [1000, 793, 618, 473, 355, 262, 190, 135, 94, 64, 43, 28, 18, 11, 7, 4, 3, 1, 1, 0];

pub fn gravity_ms(level: u32) -> u32 {
    GRAVITY_MS[(level.clamp(1, MAX_LEVEL) - 1) as usize]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Piece {
    I = 1,
    O,
    T,
    S,
    Z,
    J,
    L,
}

pub const ALL: [Piece; 7] = [Piece::I, Piece::O, Piece::T, Piece::S, Piece::Z, Piece::J, Piece::L];

impl Piece {
    pub fn from_cell(c: u8) -> Option<Piece> {
        ALL.get((c as usize).wrapping_sub(1)).copied()
    }
    /// Spawn-orientation cells inside the piece's box (y down).
    fn base(self) -> [(i8, i8); 4] {
        match self {
            Piece::I => [(0, 1), (1, 1), (2, 1), (3, 1)],
            Piece::O => [(1, 0), (2, 0), (1, 1), (2, 1)],
            Piece::T => [(1, 0), (0, 1), (1, 1), (2, 1)],
            Piece::S => [(1, 0), (2, 0), (0, 1), (1, 1)],
            Piece::Z => [(0, 0), (1, 0), (1, 1), (2, 1)],
            Piece::J => [(0, 0), (0, 1), (1, 1), (2, 1)],
            Piece::L => [(2, 0), (0, 1), (1, 1), (2, 1)],
        }
    }
    fn box_size(self) -> i8 {
        match self {
            Piece::I | Piece::O => 4,
            _ => 3,
        }
    }
    /// Cells for rotation state `rot` (0 = spawn, 1 = R, 2 = 180, 3 = L).
    /// SRS rotation is a rotation of the box about its centre.
    pub fn cells(self, rot: u8) -> [(i8, i8); 4] {
        let mut c = self.base();
        if self == Piece::O {
            return c;
        }
        let n = self.box_size();
        for _ in 0..(rot & 3) {
            for p in c.iter_mut() {
                *p = (n - 1 - p.1, p.0); // clockwise
            }
        }
        c
    }
}

/// SRS kick offsets (x right, y UP as in the guideline tables).
fn kicks(p: Piece, from: u8, to: u8) -> [(i8, i8); 5] {
    if p == Piece::I {
        match (from, to) {
            (0, 1) => [(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)],
            (1, 0) => [(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)],
            (1, 2) => [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)],
            (2, 1) => [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)],
            (2, 3) => [(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)],
            (3, 2) => [(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)],
            (3, 0) => [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)],
            (0, 3) => [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)],
            _ => [(0, 0); 5],
        }
    } else {
        match (from, to) {
            (0, 1) => [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)],
            (1, 0) => [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],
            (1, 2) => [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],
            (2, 1) => [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)],
            (2, 3) => [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],
            (3, 2) => [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)],
            (3, 0) => [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)],
            (0, 3) => [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],
            _ => [(0, 0); 5],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Active {
    pub piece: Piece,
    pub rot: u8,
    pub x: i8,
    pub y: i8,
}

impl Active {
    pub fn cells(&self) -> [(i8, i8); 4] {
        let mut c = self.piece.cells(self.rot);
        for p in c.iter_mut() {
            p.0 += self.x;
            p.1 += self.y;
        }
        c
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Title screen, waiting for Start.
    Ready,
    Playing,
    /// Line clear animation; `rows` bitmask (bit r = board row r).
    Clearing { rows: u32, elapsed: u32 },
    Paused,
    GameOver,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Start,
    LeftDown,
    LeftUp,
    RightDown,
    RightUp,
    SoftDown,
    SoftUp,
    HardDrop,
    RotateCw,
    RotateCcw,
    Hold,
    TogglePause,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TSpin {
    #[default]
    None,
    Mini,
    Full,
}

/// Result of the last lock that cleared lines or was a T-spin (for the HUD).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ClearInfo {
    pub lines: u8,
    pub tspin: TSpin,
    pub b2b: bool,
    pub combo: u8,
    pub points: u32,
    /// Remaining display time (ms).
    pub show_ms: u32,
}

/// xorshift32 (deterministic, seedable for tests/replays).
#[derive(Clone, Debug)]
pub struct Rng(u32);
impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng(if seed == 0 { 0x9E37_79B9 } else { seed })
    }
    pub fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
}

#[derive(Clone, Debug)]
pub struct Game {
    pub board: [[u8; COLS]; ROWS],
    pub cur: Option<Active>,
    pub hold: Option<Piece>,
    pub hold_used: bool,
    queue: [Piece; 14],
    qlen: usize,
    rng: Rng,
    pub score: u32,
    pub lines: u32,
    pub level: u32,
    pub start_level: u32,
    pub phase: Phase,
    pub combo: i32,
    pub b2b: bool,
    pub last: ClearInfo,
    pub pieces: u32,
    gravity_acc: u32,
    lock_ms: u32,
    lock_resets: u8,
    lowest_y: i8,
    last_rotate: bool,
    last_kick: usize,
    left_down: bool,
    right_down: bool,
    /// Auto-shift state: (direction, ms held).
    das: Option<(i8, u32)>,
    soft: bool,
    paused_from: Option<Phase>,
    /// Bumped whenever something visible changes (frontends redraw on change).
    pub version: u32,
}

impl Game {
    pub fn new(seed: u32) -> Self {
        let mut g = Game {
            board: [[0; COLS]; ROWS],
            cur: None,
            hold: None,
            hold_used: false,
            queue: [Piece::I; 14],
            qlen: 0,
            rng: Rng::new(seed),
            score: 0,
            lines: 0,
            level: 1,
            start_level: 1,
            phase: Phase::Ready,
            combo: -1,
            b2b: false,
            last: ClearInfo::default(),
            pieces: 0,
            gravity_acc: 0,
            lock_ms: 0,
            lock_resets: 0,
            lowest_y: 0,
            last_rotate: false,
            last_kick: 0,
            left_down: false,
            right_down: false,
            das: None,
            soft: false,
            paused_from: None,
            version: 0,
        };
        g.refill();
        g
    }

    /// Reset everything and start playing at `level`.
    pub fn start(&mut self, level: u32) {
        let seed = self.rng.next();
        *self = Game::new(seed);
        self.level = level.clamp(1, MAX_LEVEL);
        self.start_level = self.level;
        self.phase = Phase::Playing;
        self.spawn_next();
    }

    fn refill(&mut self) {
        while self.qlen <= 7 {
            let mut bag = ALL;
            for i in (1..7).rev() {
                let j = (self.rng.next() % (i as u32 + 1)) as usize;
                bag.swap(i, j);
            }
            for p in bag {
                self.queue[self.qlen] = p;
                self.qlen += 1;
            }
        }
    }

    fn pop_next(&mut self) -> Piece {
        let p = self.queue[0];
        self.queue.copy_within(1..self.qlen, 0);
        self.qlen -= 1;
        self.refill();
        p
    }

    /// The next `n` (<= 7) pieces.
    pub fn next_pieces(&self) -> &[Piece] {
        &self.queue[..5.min(self.qlen)]
    }

    pub fn cell(&self, x: i8, y: i8) -> u8 {
        if x < 0 || x >= COLS as i8 || y >= ROWS as i8 {
            return 0xFF; // wall/floor
        }
        if y < 0 {
            return 0;
        }
        self.board[y as usize][x as usize]
    }

    pub fn fits(&self, a: &Active) -> bool {
        a.cells().iter().all(|&(x, y)| self.cell(x, y) == 0)
    }

    fn spawn(&mut self, p: Piece) -> bool {
        let a = Active { piece: p, rot: 0, x: 3, y: if p == Piece::I { 0 } else { 1 } };
        self.cur = Some(a);
        self.gravity_acc = 0;
        self.lock_ms = 0;
        self.lock_resets = 0;
        self.lowest_y = a.y;
        self.last_rotate = false;
        self.version = self.version.wrapping_add(1);
        if !self.fits(&a) {
            self.phase = Phase::GameOver; // block out
            return false;
        }
        // Guideline: drop one row immediately if there is room.
        let d = Active { y: a.y + 1, ..a };
        if self.fits(&d) {
            self.cur = Some(d);
            self.lowest_y = d.y;
        }
        true
    }

    fn spawn_next(&mut self) {
        let p = self.pop_next();
        self.hold_used = false;
        self.pieces += 1;
        self.spawn(p);
    }

    /// Where the current piece would land.
    pub fn ghost(&self) -> Option<Active> {
        let mut g = self.cur?;
        while self.fits(&Active { y: g.y + 1, ..g }) {
            g.y += 1;
        }
        Some(g)
    }

    fn on_ground(&self) -> bool {
        self.cur.is_some_and(|a| !self.fits(&Active { y: a.y + 1, ..a }))
    }

    fn moved(&mut self) {
        // successful move/rotate: lock delay reset (limited)
        if self.on_ground() && self.lock_resets < MAX_LOCK_RESETS {
            self.lock_ms = 0;
            self.lock_resets += 1;
        }
        self.version = self.version.wrapping_add(1);
    }

    fn shift(&mut self, dx: i8) -> bool {
        let Some(a) = self.cur else { return false };
        let n = Active { x: a.x + dx, ..a };
        if self.fits(&n) {
            self.cur = Some(n);
            self.last_rotate = false;
            self.moved();
            true
        } else {
            false
        }
    }

    fn rotate(&mut self, cw: bool) -> bool {
        let Some(a) = self.cur else { return false };
        if a.piece == Piece::O {
            return false;
        }
        let to = if cw { (a.rot + 1) & 3 } else { (a.rot + 3) & 3 };
        for (i, (kx, ky)) in kicks(a.piece, a.rot, to).iter().enumerate() {
            let n = Active { rot: to, x: a.x + kx, y: a.y - ky, ..a };
            if self.fits(&n) {
                self.cur = Some(n);
                self.last_rotate = true;
                self.last_kick = i;
                self.moved();
                return true;
            }
        }
        false
    }

    /// Step down one row; returns false if blocked.
    fn fall(&mut self) -> bool {
        let Some(a) = self.cur else { return false };
        let n = Active { y: a.y + 1, ..a };
        if self.fits(&n) {
            self.cur = Some(n);
            self.last_rotate = false;
            if n.y > self.lowest_y {
                self.lowest_y = n.y;
                self.lock_resets = 0;
                self.lock_ms = 0;
            }
            self.version = self.version.wrapping_add(1);
            true
        } else {
            false
        }
    }

    fn tspin_kind(&self, a: &Active) -> TSpin {
        if a.piece != Piece::T || !self.last_rotate {
            return TSpin::None;
        }
        let occ = |dx: i8, dy: i8| self.cell(a.x + dx, a.y + dy) != 0;
        let corners = [occ(0, 0), occ(2, 0), occ(2, 2), occ(0, 2)]; // TL TR BR BL
        let n = corners.iter().filter(|&&c| c).count();
        if n < 3 {
            return TSpin::None;
        }
        // front corners (the side the T points to)
        let (f1, f2) = match a.rot {
            0 => (0, 1),
            1 => (1, 2),
            2 => (2, 3),
            _ => (3, 0),
        };
        if (corners[f1] && corners[f2]) || self.last_kick == 4 {
            TSpin::Full
        } else {
            TSpin::Mini
        }
    }

    fn lock(&mut self) {
        let Some(a) = self.cur.take() else { return };
        let tspin = self.tspin_kind(&a);
        let mut all_hidden = true;
        for (x, y) in a.cells() {
            if y >= 0 {
                self.board[y as usize][x as usize] = a.piece as u8;
            }
            if y >= HIDDEN as i8 {
                all_hidden = false;
            }
        }
        let mut rows = 0u32;
        for r in 0..ROWS {
            if self.board[r].iter().all(|&c| c != 0) {
                rows |= 1 << r;
            }
        }
        let n = rows.count_ones() as u8;
        self.score_lock(n, tspin);
        self.version = self.version.wrapping_add(1);
        if n > 0 {
            self.phase = Phase::Clearing { rows, elapsed: 0 };
        } else if all_hidden {
            self.phase = Phase::GameOver; // lock out
        } else {
            self.spawn_next();
        }
    }

    fn score_lock(&mut self, n: u8, tspin: TSpin) {
        let lvl = self.level;
        let base = match (tspin, n) {
            (TSpin::None, 1) => 100,
            (TSpin::None, 2) => 300,
            (TSpin::None, 3) => 500,
            (TSpin::None, 4) => 800,
            (TSpin::Mini, 0) => 100,
            (TSpin::Mini, 1) => 200,
            (TSpin::Mini, 2) => 400,
            (TSpin::Full, 0) => 400,
            (TSpin::Full, 1) => 800,
            (TSpin::Full, 2) => 1200,
            (TSpin::Full, 3) => 1600,
            _ => 0,
        };
        let difficult = n == 4 || (tspin != TSpin::None && n > 0);
        let mut pts = base * lvl;
        let mut b2b = false;
        if n > 0 {
            if difficult && self.b2b {
                pts = pts * 3 / 2;
                b2b = true;
            }
            self.b2b = difficult;
            self.combo += 1;
            if self.combo > 0 {
                pts += 50 * self.combo as u32 * lvl;
            }
        } else {
            self.combo = -1;
        }
        self.score += pts;
        if n > 0 || tspin != TSpin::None {
            self.last = ClearInfo {
                lines: n,
                tspin,
                b2b,
                combo: self.combo.max(0) as u8,
                points: pts,
                show_ms: 1500,
            };
        }
    }

    fn finish_clear(&mut self, rows: u32) {
        let mut out = [[0u8; COLS]; ROWS];
        let mut w = ROWS;
        for r in (0..ROWS).rev() {
            if rows & (1 << r) == 0 {
                w -= 1;
                out[w] = self.board[r];
            }
        }
        self.board = out;
        self.lines += rows.count_ones();
        let new_level = (self.start_level + self.lines / 10).min(MAX_LEVEL);
        self.level = self.level.max(new_level);
        self.phase = Phase::Playing;
        self.spawn_next();
    }

    pub fn action(&mut self, act: Action) {
        match (self.phase, act) {
            (Phase::Ready | Phase::GameOver, Action::Start | Action::HardDrop | Action::TogglePause) => {
                let lvl = self.start_level;
                self.start(lvl);
                return;
            }
            (Phase::Playing | Phase::Clearing { .. }, Action::TogglePause) => {
                self.paused_from = Some(self.phase);
                self.phase = Phase::Paused;
                self.left_down = false;
                self.right_down = false;
                self.das = None;
                self.soft = false;
                self.version = self.version.wrapping_add(1);
                return;
            }
            (Phase::Paused, Action::TogglePause | Action::Start) => {
                self.phase = self.paused_from.take().unwrap_or(Phase::Playing);
                self.version = self.version.wrapping_add(1);
                return;
            }
            (Phase::Playing, _) => {}
            // Allow DAS state changes during clear animation, ignore others
            (Phase::Clearing { .. }, Action::LeftUp | Action::RightUp | Action::SoftUp) => {}
            (Phase::Clearing { .. }, Action::LeftDown) => {
                self.left_down = true;
                self.das = Some((-1, 0));
                return;
            }
            (Phase::Clearing { .. }, Action::RightDown) => {
                self.right_down = true;
                self.das = Some((1, 0));
                return;
            }
            _ => return,
        }
        match act {
            Action::LeftDown => {
                self.left_down = true;
                self.das = Some((-1, 0));
                self.shift(-1);
            }
            Action::RightDown => {
                self.right_down = true;
                self.das = Some((1, 0));
                self.shift(1);
            }
            Action::LeftUp => {
                self.left_down = false;
                self.das = if self.right_down { Some((1, 0)) } else { None };
            }
            Action::RightUp => {
                self.right_down = false;
                self.das = if self.left_down { Some((-1, 0)) } else { None };
            }
            Action::SoftDown => {
                self.soft = true;
                if self.fall() {
                    self.score += 1;
                }
                self.gravity_acc = 0;
            }
            Action::SoftUp => self.soft = false,
            Action::HardDrop => {
                let mut n = 0;
                while self.fall() {
                    n += 1;
                }
                self.score += 2 * n;
                self.lock();
            }
            Action::RotateCw => {
                self.rotate(true);
            }
            Action::RotateCcw => {
                self.rotate(false);
            }
            Action::Hold => {
                if self.hold_used {
                    return;
                }
                let Some(a) = self.cur else { return };
                self.hold_used = true;
                match self.hold.replace(a.piece) {
                    Some(h) => {
                        self.spawn(h);
                    }
                    None => {
                        let p = self.pop_next();
                        self.spawn(p);
                    }
                }
                self.version = self.version.wrapping_add(1);
            }
            _ => {}
        }
    }

    /// Advance time by `dt` ms (call at ~60 Hz or whenever convenient).
    pub fn tick(&mut self, dt: u32) {
        if self.last.show_ms > 0 {
            let before = self.last.show_ms;
            self.last.show_ms = self.last.show_ms.saturating_sub(dt);
            if before > 0 && self.last.show_ms == 0 {
                self.version = self.version.wrapping_add(1);
            }
        }
        match self.phase {
            Phase::Clearing { rows, elapsed } => {
                let e = elapsed + dt;
                if e >= CLEAR_ANIM_MS {
                    self.finish_clear(rows);
                } else {
                    self.phase = Phase::Clearing { rows, elapsed: e };
                }
                self.version = self.version.wrapping_add(1);
                return;
            }
            Phase::Playing => {}
            _ => return,
        }
        // DAS / ARR for the most recently pressed direction
        if let Some((dir, t0)) = self.das {
            let t1 = t0 + dt;
            if t1 >= DAS_MS {
                let before = if t0 < DAS_MS { 0 } else { (t0 - DAS_MS) / ARR_MS + 1 };
                let after = (t1 - DAS_MS) / ARR_MS + 1;
                for _ in before..after {
                    if !self.shift(dir) {
                        break;
                    }
                }
            }
            self.das = Some((dir, t1));
        }
        // gravity
        let g = if self.soft { SOFT_DROP_MS.min(gravity_ms(self.level)) } else { gravity_ms(self.level) };
        if g == 0 {
            while self.fall() {}
        } else {
            self.gravity_acc += dt;
            while self.gravity_acc >= g {
                self.gravity_acc -= g;
                if self.fall() {
                    if self.soft {
                        self.score += 1;
                    }
                } else {
                    self.gravity_acc = 0;
                    break;
                }
            }
        }
        // lock delay
        if self.on_ground() {
            self.lock_ms += dt;
            if self.lock_ms >= LOCK_DELAY_MS {
                self.lock();
            }
        } else {
            self.lock_ms = 0;
        }
    }

    /// Visible row index (0 = top visible row) -> board row.
    pub fn visible_row(r: usize) -> usize {
        r + HIDDEN
    }
}

#[cfg(test)]
mod tests;
