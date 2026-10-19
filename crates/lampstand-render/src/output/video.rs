//! Which videos keep playing when the scene changes.
//!
//! A video node with the same node ID and blob in the old and new scene
//! continues from where it was, so a motion background runs on under
//! changing lyrics. The render thread keeps a `VideoClock` per output with
//! the time each playing video started; T27's players read it to pick the
//! frame and are kept or dropped by the same key.

use std::collections::HashMap;

use lampstand_core::id::BlobHash;
use lampstand_core::scene::{Node, NodeId, NodeKind, Scene};

pub type VideoKey = (NodeId, BlobHash);

/// Start time (render-thread ms) of each playing video on one output.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VideoClock {
    pub started: HashMap<VideoKey, f64>,
}

fn videos(nodes: &[Node], out: &mut Vec<(VideoKey, bool)>) {
    for node in nodes {
        match &node.kind {
            NodeKind::Video(v) => out.push(((node.id, v.blob), v.autoplay)),
            NodeKind::Group(children) => videos(children, out),
            _ => {}
        }
    }
}

impl VideoClock {
    /// Moves to `scene` at `now`: videos also in the old scene keep their
    /// start time, new autoplay videos start now, everything else stops.
    pub fn show(&mut self, scene: &Scene, now: f64) {
        let mut found = Vec::new();
        videos(&scene.nodes, &mut found);
        let mut next = HashMap::new();
        for (key, autoplay) in found {
            if let Some(&start) = self.started.get(&key) {
                next.insert(key, start);
            } else if autoplay {
                next.insert(key, now);
            }
        }
        self.started = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_core::geom::{Rect, Transform};
    use lampstand_core::scene::{Fit, VideoNode};

    fn video(id: NodeId, blob: &[u8], autoplay: bool) -> Node {
        Node {
            id,
            transform: Transform::IDENTITY,
            opacity: 1.0,
            visible: true,
            kind: NodeKind::Video(VideoNode {
                blob: BlobHash::of(blob),
                rect: Rect::default(),
                fit: Fit::Cover,
                looped: true,
                muted: true,
                autoplay,
            }),
        }
    }

    fn scene(nodes: Vec<Node>) -> Scene {
        Scene {
            width: 1920.0,
            height: 1080.0,
            nodes,
        }
    }

    #[test]
    fn a_shared_background_keeps_playing() {
        let mut clock = VideoClock::default();
        clock.show(&scene(vec![video(1, b"waves", true)]), 100.0);
        // Next slide: same background inside a group, plus new lyrics.
        let group = Node {
            kind: NodeKind::Group(vec![video(1, b"waves", true)]),
            ..video(9, b"", false)
        };
        clock.show(&scene(vec![group]), 5000.0);
        assert_eq!(
            clock.started.get(&(1, BlobHash::of(b"waves"))),
            Some(&100.0)
        );
    }

    #[test]
    fn a_different_blob_or_node_restarts() {
        let mut clock = VideoClock::default();
        clock.show(&scene(vec![video(1, b"waves", true)]), 100.0);
        clock.show(&scene(vec![video(1, b"clouds", true)]), 200.0);
        assert_eq!(
            clock.started.get(&(1, BlobHash::of(b"clouds"))),
            Some(&200.0)
        );
        assert_eq!(clock.started.len(), 1);
        clock.show(&scene(vec![video(2, b"clouds", true)]), 300.0);
        assert_eq!(
            clock.started.get(&(2, BlobHash::of(b"clouds"))),
            Some(&300.0)
        );
    }

    #[test]
    fn videos_without_autoplay_wait() {
        let mut clock = VideoClock::default();
        clock.show(&scene(vec![video(1, b"clip", false)]), 0.0);
        assert!(clock.started.is_empty());
    }
}
