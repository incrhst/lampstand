//! Keys pressed in an output window (docs/v0.1.md, "Commands").

use lampstand_render::winit::keyboard::{Key, NamedKey};
use lampstand_show::protocol::{Command, Mode};

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Send to the server.
    Command(Command),
    /// Toggle the calibration grid on this window.
    Calibrate,
}

/// The show action for a key, outside calibration.
pub fn action(key: &Key) -> Option<Action> {
    let command = match key {
        Key::Named(NamedKey::ArrowRight | NamedKey::ArrowDown | NamedKey::PageDown | NamedKey::Space) => Command::Next,
        Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowUp | NamedKey::PageUp) => Command::Prev,
        Key::Named(NamedKey::Enter) => Command::GoLive,
        Key::Character(c) => match c.to_lowercase().as_str() {
            " " => Command::Next,
            "b" | "." => Command::Mode { mode: Mode::Black },
            "w" | "," => Command::Mode { mode: Mode::Clear },
            "l" => Command::Mode { mode: Mode::Logo },
            "c" => return Some(Action::Calibrate),
            _ => return None,
        },
        _ => return None,
    };
    Some(Action::Command(command))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(s: &str) -> Key {
        Key::Character(s.into())
    }

    fn cmd(key: Key) -> Option<Command> {
        match action(&key) {
            Some(Action::Command(c)) => Some(c),
            _ => None,
        }
    }

    #[test]
    fn keys_match_the_doc() {
        for k in [NamedKey::ArrowRight, NamedKey::ArrowDown, NamedKey::PageDown, NamedKey::Space] {
            assert_eq!(cmd(Key::Named(k)), Some(Command::Next), "{k:?}");
        }
        for k in [NamedKey::ArrowLeft, NamedKey::ArrowUp, NamedKey::PageUp] {
            assert_eq!(cmd(Key::Named(k)), Some(Command::Prev), "{k:?}");
        }
        assert_eq!(cmd(Key::Named(NamedKey::Enter)), Some(Command::GoLive));
        for (k, mode) in [("b", Mode::Black), ("B", Mode::Black), (".", Mode::Black), ("w", Mode::Clear), (",", Mode::Clear), ("l", Mode::Logo)] {
            assert_eq!(cmd(ch(k)), Some(Command::Mode { mode }), "{k}");
        }
        assert_eq!(action(&ch("c")), Some(Action::Calibrate));
        assert_eq!(action(&ch("x")), None);
        assert_eq!(action(&Key::Named(NamedKey::Escape)), None);
    }
}
