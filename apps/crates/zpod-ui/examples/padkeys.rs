//! Print simulator `key` lines (ZPOD_SIM_SCRIPT) that type a text with the
//! buttons on the on-screen picker (zpod_ui::textentry), starting from a
//! fresh entry:  cargo run -q -p zpod-ui --example padkeys -- 'Abc 1!'
//! Used by apps/tools/sim/password/run.sh. Example text only: never a real
//! password.
use zpod_ui::textentry::{Cell, Pad, Picker};

fn main() {
    let text = std::env::args().nth(1).unwrap_or_default();
    let mut p = Picker::default();
    for ch in text.chars() {
        let target = if ch == ' ' { Cell::Space } else { Cell::Char(ch) };
        let Some(path) = p.presses_to(target) else {
            eprintln!("padkeys: {ch:?} is not on the picker");
            std::process::exit(1);
        };
        for m in path {
            match m {
                Pad::NextSet => p.select_set(p.set + 1),
                m => {
                    p.step(m);
                }
            }
            let name = match m {
                Pad::Up => "up",
                Pad::Down => "down",
                Pad::Left => "left",
                Pad::Right => "right",
                Pad::NextSet => "square",
                _ => unreachable!(),
            };
            println!("key {name}");
        }
        println!("key ok");
    }
}
