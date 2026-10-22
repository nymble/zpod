//! ZPOD button conventions (docs/ui/BUTTONS-AND-MENUS.md), shared by the
//! launcher and every app so menus behave the same everywhere:
//!
//! | Button | Meaning in any list/menu |
//! |---|---|
//! | Up / Down | move (auto-repeat while held) |
//! | Right, Centre (OK), Circle | enter / select |
//! | Left, X | back one level (top level of an app: exit to the launcher) |
//! | Home | global: back to the top launcher menu |
//! | Play/Pause | global: play/pause music |
//! | Vol+ / Vol- | global: volume (daemon), same overlay everywhere |
//! | Triangle, Square | app-local (unused by the launcher) |
//!
//! Apps may override Left/Right/Up/Down/Triangle/Square/Circle/X on their
//! own screens (a game, a time editor, Now Playing) but must keep Home,
//! Play/Pause and the volume keys global, and must document overrides.

use crate::{Event, Key, KeyState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Enter,
    Back,
    Home,
    PlayPause,
    Volume,
    /// Triangle / Square: app-local.
    Other(Key),
}

/// Map an event to its conventional meaning. Up/Down act on press and
/// auto-repeat; everything else only on the initial press. Releases map to
/// None (the volume overlay sees them separately).
pub fn classify(ev: &Event) -> Option<Nav> {
    let repeat_ok = matches!(ev.key, Key::Up | Key::Down);
    match ev.state {
        KeyState::Release => return None,
        KeyState::Repeat if !repeat_ok => {
            return if matches!(ev.key, Key::VolUp | Key::VolDown) { Some(Nav::Volume) } else { None };
        }
        _ => {}
    }
    Some(match ev.key {
        Key::Up => Nav::Up,
        Key::Down => Nav::Down,
        Key::Right | Key::Ok | Key::Circle => Nav::Enter,
        Key::Left | Key::Cross => Nav::Back,
        Key::Home => Nav::Home,
        Key::PlayPause => Nav::PlayPause,
        Key::VolUp | Key::VolDown => Nav::Volume,
        k @ (Key::Triangle | Key::Square) => Nav::Other(k),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(k: Key) -> Option<Nav> {
        classify(&Event { key: k, state: KeyState::Press })
    }
    fn r(k: Key) -> Option<Nav> {
        classify(&Event { key: k, state: KeyState::Repeat })
    }

    #[test]
    fn left_is_always_back_and_right_is_enter() {
        assert_eq!(p(Key::Left), Some(Nav::Back));
        assert_eq!(p(Key::Cross), Some(Nav::Back));
        assert_eq!(p(Key::Right), Some(Nav::Enter));
        assert_eq!(p(Key::Ok), Some(Nav::Enter));
        assert_eq!(p(Key::Circle), Some(Nav::Enter));
    }

    #[test]
    fn only_up_down_repeat() {
        assert_eq!(r(Key::Up), Some(Nav::Up));
        assert_eq!(r(Key::Down), Some(Nav::Down));
        assert_eq!(r(Key::Left), None);
        assert_eq!(r(Key::Ok), None);
        assert_eq!(r(Key::Home), None);
        assert_eq!(classify(&Event { key: Key::Up, state: KeyState::Release }), None);
    }

    #[test]
    fn globals() {
        assert_eq!(p(Key::Home), Some(Nav::Home));
        assert_eq!(p(Key::PlayPause), Some(Nav::PlayPause));
        assert_eq!(p(Key::VolUp), Some(Nav::Volume));
        assert_eq!(r(Key::VolDown), Some(Nav::Volume));
        assert_eq!(p(Key::Triangle), Some(Nav::Other(Key::Triangle)));
        assert_eq!(p(Key::Square), Some(Nav::Other(Key::Square)));
    }
}
