extern crate std;
use super::*;
use std::vec::Vec;

fn sorted(mut c: [(i8, i8); 4]) -> [(i8, i8); 4] {
    c.sort();
    c
}

fn playing() -> Game {
    let mut g = Game::new(42);
    g.start(1);
    g
}

fn fill_row(g: &mut Game, r: usize, holes: &[usize]) {
    for x in 0..COLS {
        g.board[r][x] = if holes.contains(&x) { 0 } else { Piece::Z as u8 };
    }
}

#[test]
fn srs_rotation_states() {
    assert_eq!(sorted(Piece::T.cells(1)), sorted([(1, 0), (1, 1), (2, 1), (1, 2)]));
    assert_eq!(sorted(Piece::T.cells(2)), sorted([(0, 1), (1, 1), (2, 1), (1, 2)]));
    assert_eq!(sorted(Piece::I.cells(1)), sorted([(2, 0), (2, 1), (2, 2), (2, 3)]));
    assert_eq!(sorted(Piece::I.cells(2)), sorted([(0, 2), (1, 2), (2, 2), (3, 2)]));
    assert_eq!(sorted(Piece::J.cells(3)), sorted([(1, 0), (1, 1), (0, 2), (1, 2)]));
    for p in ALL {
        assert_eq!(sorted(p.cells(0)), sorted(p.cells(4 & 3)));
        assert_eq!(Piece::from_cell(p as u8), Some(p));
    }
    assert_eq!(Piece::O.cells(1), Piece::O.cells(0));
}

#[test]
fn seven_bag() {
    let mut g = Game::new(7);
    for _ in 0..4 {
        let mut seen = Vec::new();
        for _ in 0..7 {
            seen.push(g.pop_next() as u8);
        }
        seen.sort();
        assert_eq!(seen, [1, 2, 3, 4, 5, 6, 7]);
    }
    assert_eq!(g.next_pieces().len(), 5);
}

#[test]
fn spawn_and_gravity() {
    let mut g = playing();
    let a = g.cur.unwrap();
    assert_eq!(a.rot, 0);
    let y0 = a.y;
    g.tick(999);
    assert_eq!(g.cur.unwrap().y, y0);
    g.tick(1);
    assert_eq!(g.cur.unwrap().y, y0 + 1);
    assert!(gravity_ms(1) > gravity_ms(10));
    assert_eq!(gravity_ms(99), 0); // 20G
}

#[test]
fn hard_drop_scores_and_locks() {
    let mut g = playing();
    let ghost = g.ghost().unwrap();
    let a = g.cur.unwrap();
    g.action(Action::HardDrop);
    let dist = (ghost.y - a.y) as u32;
    assert_eq!(g.score, 2 * dist);
    for (x, y) in ghost.cells() {
        assert_eq!(g.board[y as usize][x as usize], a.piece as u8);
    }
    assert_eq!(g.pieces, 2);
}

#[test]
fn single_and_tetris_clear() {
    let mut g = playing();
    for r in 18..22 {
        fill_row(&mut g, r, &[9]);
    }
    g.cur = Some(Active { piece: Piece::I, rot: 1, x: 7, y: 2 }); // vertical in column 9
    g.action(Action::HardDrop);
    let drop = 16; // y 2 -> 18
    assert!(matches!(g.phase, Phase::Clearing { rows, .. } if rows.count_ones() == 4));
    assert_eq!(g.last.lines, 4);
    g.tick(CLEAR_ANIM_MS);
    assert_eq!(g.phase, Phase::Playing);
    assert_eq!(g.lines, 4);
    assert_eq!(g.score, 800 + 2 * drop);
    assert!(g.board.iter().all(|r| r.iter().all(|&c| c == 0)));
    assert!(g.b2b);

    // second tetris is back-to-back: 800 * 1.5, plus combo 1 * 50
    for r in 18..22 {
        fill_row(&mut g, r, &[9]);
    }
    g.cur = Some(Active { piece: Piece::I, rot: 1, x: 7, y: 2 });
    let before = g.score;
    g.action(Action::HardDrop);
    g.tick(CLEAR_ANIM_MS);
    assert_eq!(g.score - before, 1200 + 50 + 2 * drop);
    assert!(g.last.b2b);
}

#[test]
fn level_up_every_ten_lines() {
    let mut g = playing();
    for i in 0..10 {
        g.board = [[0; COLS]; ROWS];
        fill_row(&mut g, 21, &[0, 1, 2, 3]);
        g.cur = Some(Active { piece: Piece::I, rot: 0, x: 0, y: 15 });
        g.action(Action::HardDrop);
        g.tick(CLEAR_ANIM_MS);
        assert_eq!(g.lines, i + 1);
    }
    assert_eq!(g.level, 2);
}

#[test]
fn i_wall_kick() {
    let mut g = playing();
    g.cur = Some(Active { piece: Piece::I, rot: 1, x: -2, y: 5 }); // column 0
    g.action(Action::RotateCw);
    let a = g.cur.unwrap();
    assert_eq!((a.rot, a.x), (2, 0)); // kicked right by 2 (test 3)
    assert!(a.cells().iter().all(|&(x, _)| (0..4).contains(&x)));
}

#[test]
fn t_spin_double() {
    let mut g = playing();
    g.board = [[0; COLS]; ROWS];
    g.board[19][1] = Piece::Z as u8; // overhang
    fill_row(&mut g, 20, &[1, 2, 3]);
    fill_row(&mut g, 21, &[2]);
    g.cur = Some(Active { piece: Piece::T, rot: 1, x: 1, y: 19 });
    g.action(Action::RotateCw);
    assert_eq!(g.cur.unwrap().rot, 2);
    g.action(Action::HardDrop);
    assert_eq!(g.last.tspin, TSpin::Full);
    assert_eq!(g.last.lines, 2);
    assert_eq!(g.score, 1200);
}

#[test]
fn lock_delay_and_reset_limit() {
    let mut g = playing();
    g.cur = Some(Active { piece: Piece::O, rot: 0, x: 3, y: 20 }); // resting on floor
    let pieces = g.pieces;
    g.tick(LOCK_DELAY_MS - 1);
    assert_eq!(g.pieces, pieces);
    // moves reset the delay, but only MAX_LOCK_RESETS times
    for i in 0..30 {
        g.action(if i % 2 == 0 { Action::LeftDown } else { Action::RightDown });
        g.action(Action::LeftUp);
        g.action(Action::RightUp);
        g.tick(LOCK_DELAY_MS / 2);
        if g.pieces != pieces {
            assert!(i >= MAX_LOCK_RESETS as usize, "locked early at {i}");
            return;
        }
    }
    panic!("never locked");
}

#[test]
fn hold_once_per_piece() {
    let mut g = playing();
    let first = g.cur.unwrap().piece;
    let next = g.next_pieces()[0];
    g.action(Action::Hold);
    assert_eq!(g.hold, Some(first));
    assert_eq!(g.cur.unwrap().piece, next);
    g.action(Action::Hold);
    assert_eq!(g.cur.unwrap().piece, next); // ignored
    g.action(Action::HardDrop);
    g.action(Action::Hold);
    assert_eq!(g.cur.unwrap().piece, first);
}

#[test]
fn das_auto_shift_to_wall() {
    let mut g = playing();
    g.action(Action::RightDown);
    let x1 = g.cur.unwrap().x;
    g.tick(DAS_MS - 1);
    assert_eq!(g.cur.unwrap().x, x1);
    for _ in 0..20 {
        g.tick(ARR_MS);
    }
    let a = g.cur.unwrap();
    assert_eq!(a.cells().iter().map(|c| c.0).max(), Some(9));
    g.action(Action::RightUp);
}

#[test]
fn pause_freezes_time() {
    let mut g = playing();
    let y = g.cur.unwrap().y;
    g.action(Action::TogglePause);
    assert_eq!(g.phase, Phase::Paused);
    g.tick(5000);
    assert_eq!(g.cur.unwrap().y, y);
    g.action(Action::TogglePause);
    assert_eq!(g.phase, Phase::Playing);
}

#[test]
fn game_over_on_block_out() {
    let mut g = playing();
    for r in 0..ROWS {
        fill_row(&mut g, r, &[0]);
    }
    g.cur = None;
    g.spawn_next();
    assert_eq!(g.phase, Phase::GameOver);
    g.action(Action::Start);
    assert_eq!(g.phase, Phase::Playing);
    assert_eq!(g.score, 0);
}

#[test]
fn high_scores_roundtrip() {
    use scores::*;
    let mut h = HighScores::default();
    assert_eq!(h.submit(Entry { score: 500, lines: 4, level: 1 }), Some(0));
    assert_eq!(h.submit(Entry { score: 900, lines: 9, level: 1 }), Some(0));
    assert_eq!(h.submit(Entry { score: 700, lines: 7, level: 1 }), Some(1));
    let mut buf = [0u8; 256];
    let n = h.write(&mut buf);
    let p = HighScores::parse(core::str::from_utf8(&buf[..n]).unwrap());
    assert_eq!(p, h);
    assert_eq!(p.best(), 900);
    for s in 1..10 {
        h.submit(Entry { score: 1000 + s, lines: 0, level: 1 });
    }
    assert_eq!(h.submit(Entry { score: 1, lines: 0, level: 1 }), None);
}

#[test]
fn random_play_never_panics() {
    // fuzz: random actions and ticks over many games
    let mut rng = Rng::new(1234);
    let acts = [
        Action::LeftDown, Action::LeftUp, Action::RightDown, Action::RightUp, Action::SoftDown, Action::SoftUp,
        Action::HardDrop, Action::RotateCw, Action::RotateCcw, Action::Hold,
    ];
    let mut g = playing();
    let mut games = 0;
    for _ in 0..200_000 {
        if g.phase == Phase::GameOver {
            games += 1;
            g.action(Action::Start);
        }
        let r = rng.next();
        if r % 3 == 0 {
            g.tick(r % 40);
        } else {
            g.action(acts[(r as usize / 3) % acts.len()]);
        }
        if let Some(a) = g.cur {
            if g.phase == Phase::Playing {
                assert!(g.fits(&a));
            }
        }
    }
    assert!(games > 0);
}
