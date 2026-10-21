//! The app's side of the browser source: one key and one scene feed per
//! output. The app calls [`SceneHub::publish`] with the stream output's
//! composed scene whenever it changes and delegates three `Backend`
//! methods to the hub.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use lampstand_core::output::OutputId;
use lampstand_core::scene::Scene;

/// Keys and scenes of the outputs that have a browser-source URL.
#[derive(Default)]
pub struct SceneHub {
    outputs: Mutex<HashMap<OutputId, Slot>>,
}

struct Slot {
    key: String,
    current: Option<Arc<Scene>>,
    watchers: Vec<Sender<Arc<Scene>>>,
}

impl Slot {
    fn new(key: String) -> Self {
        Self {
            key,
            current: None,
            watchers: Vec::new(),
        }
    }
}

fn fresh_key() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// At least 16 characters of URL-safe text: what `fresh_key` makes, and
/// nothing that needs escaping in a URL.
fn valid_key(key: &str) -> bool {
    key.len() >= 16
        && key.len() <= 128
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl SceneHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Gives `output` a browser-source URL and returns its key, a fresh
    /// random one the first time and the same one after.
    pub fn add_output(&self, output: OutputId) -> String {
        let mut outputs = self.outputs.lock().unwrap();
        outputs
            .entry(output)
            .or_insert_with(|| Slot::new(fresh_key()))
            .key
            .clone()
    }

    /// Like [`SceneHub::add_output`], with a key saved from an earlier run,
    /// so the URL pasted into OBS keeps working after the app restarts. A
    /// saved key that is too short or has odd characters is replaced by a
    /// fresh one; the key in use is returned, so store that. If the output
    /// already has a URL, its key becomes `saved`.
    pub fn add_output_with_key(&self, output: OutputId, saved: &str) -> String {
        let key = if valid_key(saved) {
            saved.to_string()
        } else {
            fresh_key()
        };
        let mut outputs = self.outputs.lock().unwrap();
        match outputs.get_mut(&output) {
            Some(slot) => slot.key = key.clone(),
            None => {
                outputs.insert(output, Slot::new(key.clone()));
            }
        }
        key
    }

    /// Takes the URL away: its key stops working and connected pages are
    /// disconnected (their feed closes; a later `add_output` starts a new
    /// one).
    pub fn remove_output(&self, output: OutputId) {
        self.outputs.lock().unwrap().remove(&output);
    }

    /// Replaces the key, so a leaked URL stops working. Pages already
    /// connected stay connected until they reload.
    pub fn rotate_key(&self, output: OutputId) -> Option<String> {
        let mut outputs = self.outputs.lock().unwrap();
        let slot = outputs.get_mut(&output)?;
        slot.key = fresh_key();
        Some(slot.key.clone())
    }

    /// `Backend::output_key_valid`.
    pub fn key_valid(&self, output: OutputId, key: &str) -> bool {
        let outputs = self.outputs.lock().unwrap();
        outputs
            .get(&output)
            .is_some_and(|slot| same(slot.key.as_bytes(), key.as_bytes()))
    }

    /// Shows `scene` on `output`'s browser sources. Does nothing for an
    /// output without a URL.
    pub fn publish(&self, output: OutputId, scene: Arc<Scene>) {
        let mut outputs = self.outputs.lock().unwrap();
        let Some(slot) = outputs.get_mut(&output) else {
            return;
        };
        slot.watchers.retain(|w| w.send(scene.clone()).is_ok());
        slot.current = Some(scene);
    }

    /// How many pages (OBS browser sources) are watching `output` now. A
    /// page that went away is only noticed when something is sent to it, so
    /// this re-sends the current scene (a repeat is harmless: the page
    /// draws the same thing) and forgets the pages that fail to take it.
    pub fn watcher_count(&self, output: OutputId) -> usize {
        let mut outputs = self.outputs.lock().unwrap();
        let Some(slot) = outputs.get_mut(&output) else {
            return 0;
        };
        if let Some(scene) = slot.current.clone() {
            slot.watchers.retain(|w| w.send(scene.clone()).is_ok());
        }
        slot.watchers.len()
    }

    /// `Backend::watch_scene`: the current scene (if there is one) at once,
    /// then every one published after it.
    pub fn watch(&self, output: OutputId) -> Option<Receiver<Arc<Scene>>> {
        let mut outputs = self.outputs.lock().unwrap();
        let slot = outputs.get_mut(&output)?;
        let (tx, rx) = mpsc::channel();
        if let Some(scene) = &slot.current {
            let _ = tx.send(scene.clone());
        }
        slot.watchers.push(tx);
        Some(rx)
    }
}

/// Compares without stopping at the first difference.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(width: f32) -> Arc<Scene> {
        Arc::new(Scene {
            width,
            height: 10.0,
            nodes: Vec::new(),
        })
    }

    #[test]
    fn keys_are_per_output_and_stable() {
        let hub = SceneHub::new();
        let (a, b) = (OutputId::generate(), OutputId::generate());
        let ka = hub.add_output(a);
        assert_eq!(hub.add_output(a), ka);
        let kb = hub.add_output(b);
        assert_ne!(ka, kb);
        assert!(hub.key_valid(a, &ka) && !hub.key_valid(a, &kb) && !hub.key_valid(a, ""));
        assert!(!hub.key_valid(OutputId::generate(), &ka));
        let rotated = hub.rotate_key(a).unwrap();
        assert!(hub.key_valid(a, &rotated) && !hub.key_valid(a, &ka));
        hub.remove_output(a);
        assert!(!hub.key_valid(a, &rotated) && hub.watch(a).is_none());
    }

    #[test]
    fn a_saved_key_is_restored_unless_it_is_unusable() {
        let hub = SceneHub::new();
        let out = OutputId::generate();
        let saved = "0123456789abcdefABCDEF-_";
        assert_eq!(hub.add_output_with_key(out, saved), saved);
        assert!(hub.key_valid(out, saved));
        // A restart with the same key gives the same URL.
        let again = SceneHub::new();
        assert_eq!(again.add_output_with_key(out, saved), saved);
        for bad in [
            "",
            "short",
            "has spaces in it, too long to be ok",
            "a/b?c=d&e=f#g1234",
        ] {
            let key = again.add_output_with_key(OutputId::generate(), bad);
            assert_ne!(key, bad);
            assert!(valid_key(&key));
        }
        // Setting a key on an output that has one replaces it.
        let replaced = hub.add_output_with_key(out, "another-saved-key-1234");
        assert!(hub.key_valid(out, &replaced) && !hub.key_valid(out, saved));
    }

    #[test]
    fn watchers_are_counted_and_a_closed_page_is_forgotten() {
        let hub = SceneHub::new();
        let out = OutputId::generate();
        assert_eq!(hub.watcher_count(out), 0, "no URL yet");
        hub.add_output(out);
        assert_eq!(hub.watcher_count(out), 0);
        hub.publish(out, scene(1.0));
        let first = hub.watch(out).unwrap();
        let second = hub.watch(out).unwrap();
        assert_eq!(hub.watcher_count(out), 2);
        drop(first);
        assert_eq!(
            hub.watcher_count(out),
            1,
            "a page that closed is not counted"
        );
        drop(second);
        assert_eq!(hub.watcher_count(out), 0);
    }

    #[test]
    fn a_watcher_gets_the_current_scene_then_each_change() {
        let hub = SceneHub::new();
        let out = OutputId::generate();
        hub.publish(out, scene(1.0)); // No URL yet: dropped.
        hub.add_output(out);
        let empty = hub.watch(out).unwrap();
        assert!(empty.try_recv().is_err());
        hub.publish(out, scene(2.0));
        let late = hub.watch(out).unwrap();
        hub.publish(out, scene(3.0));
        let widths = |rx: &Receiver<Arc<Scene>>| rx.try_iter().map(|s| s.width).collect::<Vec<_>>();
        assert_eq!(widths(&empty), [2.0, 3.0]);
        assert_eq!(widths(&late), [2.0, 3.0]);
    }
}
