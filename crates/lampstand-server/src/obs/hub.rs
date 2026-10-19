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
            .or_insert_with(|| Slot {
                key: uuid::Uuid::new_v4().simple().to_string(),
                current: None,
                watchers: Vec::new(),
            })
            .key
            .clone()
    }

    /// Takes the URL away: its key stops working and connected pages are
    /// disconnected.
    pub fn remove_output(&self, output: OutputId) {
        self.outputs.lock().unwrap().remove(&output);
    }

    /// Replaces the key, so a leaked URL stops working. Pages already
    /// connected stay connected until they reload.
    pub fn rotate_key(&self, output: OutputId) -> Option<String> {
        let mut outputs = self.outputs.lock().unwrap();
        let slot = outputs.get_mut(&output)?;
        slot.key = uuid::Uuid::new_v4().simple().to_string();
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
