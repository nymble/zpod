//! Tetris renderer for any `DrawTarget<Color = Rgb565>` (320x240 layout).
//!
//! The field is redrawn when the game's `version` changes; side panels only
//! when their values change. With the ZPOD framebuffer's dirty rectangles
//! and the LCD's shadow diff, only changed pixels reach the SPI bus.

use alloc::format;
use embedded_graphics::{pixelcolor::Rgb565, prelude::*};
use zpod_ui::{fill, text, text::Align, theme, Canvas, Font};

use crate::{Game, Phase, Piece, TSpin, COLS, HIDDEN, ROWS};

pub const CELL: i32 = 11;
pub const FX: i32 = 105;
pub const FY: i32 = 20;
pub const FW: i32 = CELL * COLS as i32;
pub const FH: i32 = CELL * (ROWS - HIDDEN) as i32;
const MINI: i32 = 8;

pub fn piece_color(p: Piece) -> Rgb565 {
    use theme::rgb;
    match p {
        Piece::I => rgb(0, 220, 240),
        Piece::O => rgb(250, 220, 0),
        Piece::T => rgb(170, 60, 230),
        Piece::S => rgb(40, 210, 70),
        Piece::Z => rgb(240, 50, 50),
        Piece::J => rgb(40, 100, 250),
        Piece::L => rgb(250, 140, 0),
    }
}

fn shade(c: Rgb565, num: u16, den: u16) -> Rgb565 {
    let f = |v: u8, max: u16| ((v as u16 * num / den).min(max)) as u8;
    Rgb565::new(f(c.r(), 31), f(c.g(), 63), f(c.b(), 31))
}

fn lighten(c: Rgb565) -> Rgb565 {
    Rgb565::new(c.r() + (31 - c.r()) / 2, c.g() + (63 - c.g()) / 2, c.b() + (31 - c.b()) / 2)
}

/// Bevelled block.
pub fn block<D: Canvas>(d: &mut D, x: i32, y: i32, s: i32, c: Rgb565) {
    let su = s as u32;
    fill(d, x, y, su, su, shade(c, 3, 5));
    fill(d, x, y, su - 1, su - 1, lighten(c));
    fill(d, x + 1, y + 1, su - 2, su - 2, c);
    fill(d, x + 2, y + 2, (s - 5).max(1) as u32, 1, lighten(c));
}

fn ghost_block<D: Canvas>(d: &mut D, x: i32, y: i32, s: i32, c: Rgb565) {
    let su = s as u32;
    let g = shade(c, 1, 2);
    fill(d, x, y, su, 1, g);
    fill(d, x, y + s - 1, su, 1, g);
    fill(d, x, y, 1, su, g);
    fill(d, x + s - 1, y, 1, su, g);
}

#[derive(Default, Clone, PartialEq)]
struct PanelState {
    score: u32,
    level: u32,
    lines: u32,
    best: u32,
    hold: Option<Piece>,
    hold_used: bool,
    next: [Option<Piece>; 5],
}

#[derive(Default)]
pub struct Renderer {
    drawn_version: Option<u32>,
    drawn_phase: Option<core::mem::Discriminant<Phase>>,
    panel: Option<PanelState>,
    pub status_height: i32,
}

impl Renderer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Force a complete redraw (after another app used the screen).
    pub fn invalidate(&mut self) {
        self.drawn_version = None;
        self.panel = None;
        self.drawn_phase = None;
    }

    /// Draw whatever changed. Returns true if anything was drawn.
    pub fn draw<D: Canvas>(&mut self, d: &mut D, g: &Game, best: u32) -> bool {
        let mut drew = false;
        // Overlays (title/pause/game over) spill over the side panels: a
        // phase change repaints everything.
        let ph = core::mem::discriminant(&g.phase);
        if self.drawn_phase != Some(ph) {
            self.drawn_phase = Some(ph);
            self.panel = None;
            self.drawn_version = None;
        }
        if self.panel.is_none() {
            // background + frames once
            fill(d, 0, FY, 320, (240 - FY) as u32, theme::BG);
            fill(d, FX - 2, FY, 2, FH as u32, theme::FAINT);
            fill(d, FX + FW, FY, 2, FH as u32, theme::FAINT);
        }
        let mut next = [None; 5];
        for (i, p) in g.next_pieces().iter().enumerate() {
            next[i] = Some(*p);
        }
        let ps = PanelState {
            score: g.score,
            level: g.level,
            lines: g.lines,
            best: best.max(g.score),
            hold: g.hold,
            hold_used: g.hold_used,
            next,
        };
        if self.panel.as_ref() != Some(&ps) {
            self.draw_left(d, &ps);
            self.draw_right(d, &ps);
            self.panel = Some(ps);
            drew = true;
        }
        if self.drawn_version != Some(g.version) {
            self.draw_field(d, g);
            self.drawn_version = Some(g.version);
            drew = true;
        }
        drew
    }

    fn draw_left<D: Canvas>(&self, d: &mut D, ps: &PanelState) {
        let x = 4;
        fill(d, 0, FY, (FX - 2) as u32, (240 - FY) as u32, theme::BG);
        text::draw(d, Font::BodyBold, "HOLD", x + 2, FY + 6, theme::DIM);
        fill(d, x, FY + 22, 92, 40, theme::PANEL);
        if let Some(p) = ps.hold {
            mini_piece(d, p, x + 46, FY + 42, if ps.hold_used { Some(theme::FAINT) } else { None });
        }
        let mut y = FY + 72;
        for (label, v) in [("SCORE", ps.score), ("LEVEL", ps.level), ("LINES", ps.lines), ("BEST", ps.best)] {
            text::draw(d, Font::Small, label, x + 2, y, theme::DIM);
            let s = format!("{v}");
            text::draw_aligned(d, Font::Digits20, &s, x + 94, y + 13, Align::Right, if label == "BEST" { theme::ACCENT } else { theme::FG });
            y += 38;
        }
    }

    fn draw_right<D: Canvas>(&self, d: &mut D, ps: &PanelState) {
        let x0 = FX + FW + 2;
        fill(d, x0, FY, (320 - x0) as u32, (240 - FY) as u32, theme::BG);
        text::draw(d, Font::BodyBold, "NEXT", x0 + 10, FY + 6, theme::DIM);
        fill(d, x0 + 6, FY + 22, 92, 196, theme::PANEL);
        let cx = x0 + 52;
        let mut y = FY + 44;
        for (i, p) in ps.next.iter().enumerate() {
            if let Some(p) = p {
                mini_piece(d, *p, cx, y, if i == 0 { None } else { Some(shade(piece_color(*p), 4, 5)) });
            }
            y += if i == 0 { 42 } else { 36 };
        }
    }

    fn draw_field<D: Canvas>(&self, d: &mut D, g: &Game) {
        fill(d, FX, FY, FW as u32, FH as u32, theme::BLACK);
        // subtle grid dots
        for r in 1..(ROWS - HIDDEN) as i32 {
            for c in 1..COLS as i32 {
                fill(d, FX + c * CELL, FY + r * CELL, 1, 1, theme::rgb(30, 32, 48));
            }
        }
        let (clear_rows, t) = match g.phase {
            Phase::Clearing { rows, elapsed } => (rows, elapsed),
            _ => (0, 0),
        };
        for r in HIDDEN..ROWS {
            let y = FY + (r - HIDDEN) as i32 * CELL;
            let clearing = clear_rows & (1 << r) != 0;
            for c in 0..COLS {
                let x = FX + c as i32 * CELL;
                if let Some(p) = Piece::from_cell(g.board[r][c]) {
                    if clearing {
                        // flash white, then wipe out from the centre
                        let k = t * 2 / crate::CLEAR_ANIM_MS.max(1); // 0..1 = flash, 1 = wipe
                        if k == 0 {
                            block(d, x, y, CELL, if (t / 60) % 2 == 0 { theme::WHITE } else { piece_color(p) });
                        } else {
                            let half = (t - crate::CLEAR_ANIM_MS / 2) * (COLS as u32 / 2 + 1) / (crate::CLEAR_ANIM_MS / 2);
                            let dist = if c < COLS / 2 { COLS / 2 - 1 - c } else { c - COLS / 2 } as u32;
                            if dist >= half {
                                block(d, x, y, CELL, theme::WHITE);
                            }
                        }
                    } else {
                        block(d, x, y, CELL, piece_color(p));
                    }
                }
            }
        }
        if g.phase == Phase::Playing {
            if let (Some(ghost), Some(cur)) = (g.ghost(), g.cur) {
                let c = piece_color(cur.piece);
                for (x, y) in ghost.cells() {
                    if y >= HIDDEN as i8 {
                        ghost_block(d, FX + x as i32 * CELL, FY + (y as i32 - HIDDEN as i32) * CELL, CELL, c);
                    }
                }
                for (x, y) in cur.cells() {
                    if y >= HIDDEN as i8 {
                        block(d, FX + x as i32 * CELL, FY + (y as i32 - HIDDEN as i32) * CELL, CELL, c);
                    }
                }
            }
        }
        // HUD flash for clears / T-spins
        if g.last.show_ms > 0 && matches!(g.phase, Phase::Playing | Phase::Clearing { .. }) {
            let name = match (g.last.tspin, g.last.lines) {
                (TSpin::Full, 0) => "T-SPIN",
                (TSpin::Full, 1) => "T-SPIN SINGLE",
                (TSpin::Full, 2) => "T-SPIN DOUBLE",
                (TSpin::Full, _) => "T-SPIN TRIPLE",
                (TSpin::Mini, 0) => "MINI T-SPIN",
                (TSpin::Mini, _) => "MINI T-SPIN",
                (_, 4) => "TETRIS!",
                (_, 3) => "TRIPLE",
                (_, 2) => "DOUBLE",
                (_, 1) => "SINGLE",
                _ => "",
            };
            if !name.is_empty() && g.last.lines != 1 || g.last.tspin != TSpin::None {
                text::draw_aligned(d, Font::BodyBold, name, FX + FW / 2, FY + 30, Align::Center, theme::ACCENT);
                let mut y = FY + 46;
                if g.last.b2b {
                    text::draw_aligned(d, Font::Small, "BACK-TO-BACK", FX + FW / 2, y, Align::Center, theme::CYAN);
                    y += 14;
                }
                if g.last.combo > 0 {
                    let s = format!("COMBO x{}", g.last.combo);
                    text::draw_aligned(d, Font::Small, &s, FX + FW / 2, y, Align::Center, theme::OK);
                }
            }
        }
        match g.phase {
            Phase::Paused => overlay(d, "PAUSED", &["Play/Pause: resume", "Home: quit"]),
            Phase::GameOver => {
                let s = format!("Score {}", g.score);
                overlay(d, "GAME OVER", &[&s, "OK / Play: new game", "Home: quit"]);
            }
            Phase::Ready => overlay(d, "TETRIS", &["OK or Play: start", "Up drop, Down soft drop", "Circle/Triangle rotate", "Square hold"]),
            _ => {}
        }
    }
}

fn overlay<D: Canvas>(d: &mut D, title: &str, lines: &[&str]) {
    let h = 40 + lines.len() as i32 * 16;
    let y = FY + (FH - h) / 2;
    fill(d, FX - 30, y, (FW + 60) as u32, h as u32, theme::PANEL);
    fill(d, FX - 30, y, (FW + 60) as u32, 2, theme::ACCENT);
    fill(d, FX - 30, y + h - 2, (FW + 60) as u32, 2, theme::ACCENT);
    text::draw_aligned(d, Font::Large, title, FX + FW / 2, y + 10, Align::Center, theme::ACCENT);
    let mut ly = y + 36;
    for l in lines {
        text::draw_aligned(d, Font::Small, l, FX + FW / 2, ly, Align::Center, theme::FG);
        ly += 16;
    }
}

/// Piece centred at (cx, cy) with mini cells.
pub fn mini_piece<D: Canvas>(d: &mut D, p: Piece, cx: i32, cy: i32, tint: Option<Rgb565>) {
    let cells = p.cells(0);
    let (mut minx, mut maxx, mut miny, mut maxy) = (9, -9, 9, -9);
    for (x, y) in cells {
        minx = minx.min(x);
        maxx = maxx.max(x);
        miny = miny.min(y);
        maxy = maxy.max(y);
    }
    let w = (maxx - minx + 1) as i32 * MINI;
    let h = (maxy - miny + 1) as i32 * MINI;
    let c = tint.unwrap_or(piece_color(p));
    for (x, y) in cells {
        block(d, cx - w / 2 + (x - minx) as i32 * MINI, cy - h / 2 + (y - miny) as i32 * MINI, MINI, c);
    }
}
