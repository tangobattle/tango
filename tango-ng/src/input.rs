//! Keyboard → GBA joyflag mapping. Fixed layout for now, matching the
//! tango crate's defaults (Z=A, X=B, A=L, S=R, Enter=Start, Space=Select,
//! arrows for the pad, Shift=fast-forward); the configurable mapping comes
//! with the settings pane.

use mgba::input::keys;

pub enum KeyAction {
    Joyflag(u32),
    FastForward,
    EndSession,
}

/// Classify one Slint key-event `text`. Slint delivers special keys as
/// single private-use-area characters exposed by `slint::platform::Key`.
pub fn classify(text: &str) -> Option<KeyAction> {
    use slint::platform::Key;
    let c = text.chars().next()?;
    Some(if c == char::from(Key::Return) {
        KeyAction::Joyflag(keys::START)
    } else if c == char::from(Key::UpArrow) {
        KeyAction::Joyflag(keys::UP)
    } else if c == char::from(Key::DownArrow) {
        KeyAction::Joyflag(keys::DOWN)
    } else if c == char::from(Key::LeftArrow) {
        KeyAction::Joyflag(keys::LEFT)
    } else if c == char::from(Key::RightArrow) {
        KeyAction::Joyflag(keys::RIGHT)
    } else if c == char::from(Key::Shift) {
        KeyAction::FastForward
    } else if c == char::from(Key::Escape) {
        KeyAction::EndSession
    } else {
        match c.to_ascii_lowercase() {
            'z' => KeyAction::Joyflag(keys::A),
            'x' => KeyAction::Joyflag(keys::B),
            'a' => KeyAction::Joyflag(keys::L),
            's' => KeyAction::Joyflag(keys::R),
            ' ' => KeyAction::Joyflag(keys::SELECT),
            _ => return None,
        }
    })
}
