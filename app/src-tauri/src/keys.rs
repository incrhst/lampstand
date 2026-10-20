//! Clicker mode and the keys Lampstand takes from the system (T56, PS-9).
//!
//! Presenter clickers send Page Up, Page Down, B and full stop. In clicker
//! mode, while a service is open for live, those four are registered as
//! global shortcuts (`tauri-plugin-global-shortcut`), so a clicker works
//! whichever window has focus: the operator window, an output window, or
//! another app entirely. While they are held, B means Black, as clickers
//! intend, rather than the operator view's Bridge.
//!
//! The operator window handles its own keys in the web UI. Output windows
//! have no webview, so they cannot see key presses; while one of them has
//! focus, the arrow keys (and Page Up, Page Down, full stop and Space) are
//! registered too, and released as soon as it loses focus, so other apps
//! keep their arrow keys.
//!
//! `Keys` is the registration lifecycle, with the shortcut API behind
//! `ShortcutHost` so tests drive it with a fake: the shell calls `update`
//! with the current `KeyState` after every app-thread step, and only the
//! difference is registered or released.

use std::collections::BTreeSet;

use lampstand_live::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    PageUp,
    PageDown,
    B,
    Period,
    Left,
    Right,
    Space,
}

impl Key {
    /// What a presenter clicker sends.
    pub const CLICKER: [Key; 4] = [Key::PageUp, Key::PageDown, Key::B, Key::Period];

    /// The operator keys an output window takes while it has focus. B is
    /// left out: in Lampstand's windows it means Bridge, which needs the
    /// operator view.
    pub const OUTPUT_WINDOW: [Key; 6] = [
        Key::Left,
        Key::Right,
        Key::Space,
        Key::PageUp,
        Key::PageDown,
        Key::Period,
    ];

    pub const ALL: [Key; 7] = [
        Key::PageUp,
        Key::PageDown,
        Key::B,
        Key::Period,
        Key::Left,
        Key::Right,
        Key::Space,
    ];

    /// The live command a press sends.
    pub fn command(self) -> Command {
        match self {
            Key::PageDown | Key::Right | Key::Space => Command::Next,
            Key::PageUp | Key::Left => Command::Previous,
            Key::B | Key::Period => Command::ToggleBlack,
        }
    }
}

/// What decides which keys are held.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyState {
    /// The operator turned clicker mode on (`DeviceSettings`).
    pub clicker_mode: bool,
    /// A service is open for live.
    pub live: bool,
    /// One of Lampstand's output windows has focus.
    pub output_focused: bool,
}

impl KeyState {
    pub fn wanted(&self) -> BTreeSet<Key> {
        let mut keys = BTreeSet::new();
        if self.clicker_mode && self.live {
            keys.extend(Key::CLICKER);
        }
        if self.output_focused {
            keys.extend(Key::OUTPUT_WINDOW);
        }
        keys
    }
}

/// Registers and releases global shortcuts. The Tauri implementation is
/// in `shell`.
pub trait ShortcutHost {
    fn register(&mut self, key: Key) -> Result<(), String>;
    fn unregister(&mut self, key: Key) -> Result<(), String>;
}

pub struct Keys<H> {
    host: H,
    held: BTreeSet<Key>,
    /// The state last applied. A key another app holds is tried again only
    /// when this changes, not on every step.
    state: Option<KeyState>,
}

impl<H: ShortcutHost> Keys<H> {
    pub fn new(host: H) -> Self {
        Self {
            host,
            held: BTreeSet::new(),
            state: None,
        }
    }

    /// The keys registered now.
    pub fn held(&self) -> &BTreeSet<Key> {
        &self.held
    }

    pub fn host(&self) -> &H {
        &self.host
    }

    /// Registers the keys `state` wants and releases the rest.
    pub fn update(&mut self, state: KeyState) {
        if self.state == Some(state) {
            return;
        }
        self.state = Some(state);
        let wanted = state.wanted();
        let release: Vec<Key> = self.held.difference(&wanted).copied().collect();
        for key in release {
            if let Err(e) = self.host.unregister(key) {
                eprintln!("lampstand: cannot release {key:?}: {e}");
            }
            self.held.remove(&key);
        }
        let take: Vec<Key> = wanted.difference(&self.held).copied().collect();
        for key in take {
            match self.host.register(key) {
                Ok(()) => {
                    self.held.insert(key);
                }
                // Usually another app holds it.
                Err(e) => eprintln!("lampstand: cannot take {key:?} for the clicker: {e}"),
            }
        }
    }

    /// Releases every key, as on quitting.
    pub fn release_all(&mut self) {
        self.update(KeyState::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records calls; refuses the keys in `taken`, as when another app
    /// holds them.
    #[derive(Default)]
    struct Fake {
        registered: BTreeSet<Key>,
        calls: Vec<(bool, Key)>,
        taken: BTreeSet<Key>,
    }

    impl ShortcutHost for Fake {
        fn register(&mut self, key: Key) -> Result<(), String> {
            self.calls.push((true, key));
            if self.taken.contains(&key) {
                return Err("already registered".into());
            }
            assert!(self.registered.insert(key), "{key:?} registered twice");
            Ok(())
        }
        fn unregister(&mut self, key: Key) -> Result<(), String> {
            self.calls.push((false, key));
            assert!(self.registered.remove(&key), "{key:?} was not registered");
            Ok(())
        }
    }

    fn state(clicker_mode: bool, live: bool, output_focused: bool) -> KeyState {
        KeyState {
            clicker_mode,
            live,
            output_focused,
        }
    }

    fn set(keys: &[Key]) -> BTreeSet<Key> {
        keys.iter().copied().collect()
    }

    #[test]
    fn nothing_is_taken_until_clicker_mode_is_on_and_a_service_is_live() {
        let mut keys = Keys::new(Fake::default());
        keys.update(state(false, false, false));
        keys.update(state(false, true, false));
        keys.update(state(true, false, false));
        assert!(keys.held().is_empty());
        assert!(keys.host().calls.is_empty());

        keys.update(state(true, true, false));
        assert_eq!(*keys.held(), set(&Key::CLICKER));
        assert_eq!(keys.host().registered, set(&Key::CLICKER));
    }

    #[test]
    fn turning_clicker_mode_off_or_closing_the_service_releases_them() {
        let mut keys = Keys::new(Fake::default());
        keys.update(state(true, true, false));
        keys.update(state(false, true, false));
        assert!(keys.held().is_empty());
        assert!(keys.host().registered.is_empty());

        keys.update(state(true, true, false));
        keys.update(state(true, false, false));
        assert!(keys.host().registered.is_empty());
    }

    #[test]
    fn an_unchanged_state_registers_nothing_again() {
        let mut keys = Keys::new(Fake::default());
        for _ in 0..10 {
            keys.update(state(true, true, false));
        }
        assert_eq!(keys.host().calls.len(), Key::CLICKER.len());
    }

    #[test]
    fn arrows_are_taken_only_while_an_output_window_has_focus() {
        let mut keys = Keys::new(Fake::default());
        keys.update(state(false, true, true));
        assert_eq!(*keys.held(), set(&Key::OUTPUT_WINDOW));
        assert!(keys.held().contains(&Key::Left));
        assert!(
            !keys.held().contains(&Key::B),
            "B is Bridge in Lampstand's windows"
        );
        keys.update(state(false, true, false));
        assert!(keys.host().registered.is_empty());
    }

    #[test]
    fn clicker_keys_stay_held_when_an_output_window_loses_focus() {
        let mut keys = Keys::new(Fake::default());
        keys.update(state(true, true, true));
        let mut both = set(&Key::CLICKER);
        both.extend(Key::OUTPUT_WINDOW);
        assert_eq!(*keys.held(), both);

        keys.host.calls.clear();
        keys.update(state(true, true, false));
        assert_eq!(*keys.held(), set(&Key::CLICKER));
        // Only the arrows and Space were released; Page Up stayed put.
        let released: BTreeSet<Key> = keys.host().calls.iter().map(|(_, k)| *k).collect();
        assert_eq!(released, set(&[Key::Left, Key::Right, Key::Space]));
        assert!(keys.host().calls.iter().all(|(register, _)| !register));
    }

    #[test]
    fn a_key_another_app_holds_is_skipped_and_tried_again_on_the_next_change() {
        let mut keys = Keys::new(Fake {
            taken: set(&[Key::B]),
            ..Fake::default()
        });
        keys.update(state(true, true, false));
        assert_eq!(
            *keys.held(),
            set(&[Key::PageUp, Key::PageDown, Key::Period])
        );
        let tries = |keys: &Keys<Fake>| {
            keys.host()
                .calls
                .iter()
                .filter(|c| **c == (true, Key::B))
                .count()
        };
        keys.update(state(true, true, false));
        assert_eq!(tries(&keys), 1, "not retried while nothing changed");

        keys.host.taken.clear();
        keys.update(state(true, true, true));
        assert_eq!(tries(&keys), 2);
        assert!(keys.held().contains(&Key::B));
    }

    #[test]
    fn release_all_lets_everything_go() {
        let mut keys = Keys::new(Fake::default());
        keys.update(state(true, true, true));
        keys.release_all();
        assert!(keys.held().is_empty());
        assert!(keys.host().registered.is_empty());
    }

    #[test]
    fn clicker_keys_move_and_black() {
        assert_eq!(Key::PageDown.command(), Command::Next);
        assert_eq!(Key::PageUp.command(), Command::Previous);
        assert_eq!(Key::B.command(), Command::ToggleBlack);
        assert_eq!(Key::Period.command(), Command::ToggleBlack);
        assert_eq!(Key::Right.command(), Command::Next);
        assert_eq!(Key::Left.command(), Command::Previous);
        assert_eq!(Key::Space.command(), Command::Next);
    }
}
