//! Video playback on the outputs (T27).
//!
//! A video node with the same node ID and blob in the old and new scene
//! continues from where it was, so a motion background runs on under
//! changing lyrics. The render thread keeps a `VideoClock` per output with
//! the time each playing video started, and one `Players` for all outputs:
//! a decoder and a texture per playing video, keyed by the file and that
//! start time, so outputs that started the same file together share one
//! decoder and stay in step.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;

use lampstand_core::id::BlobHash;
use lampstand_core::scene::{Node, NodeId, NodeKind, Scene};
use lampstand_media::{Frame, MediaError, Sound, Video};

use crate::gpu::{Gpu, Texture};
use crate::BlobSource;

pub type VideoKey = (NodeId, BlobHash);

/// Start time (render-thread ms) of each playing video on one output.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VideoClock {
    pub started: HashMap<VideoKey, f64>,
    /// The playing videos that loop.
    pub looped: HashSet<VideoKey>,
    /// The playing videos whose node is not muted.
    pub sounding: HashSet<VideoKey>,
}

fn videos(nodes: &[Node], out: &mut Vec<(VideoKey, bool, bool, bool)>) {
    for node in nodes {
        match &node.kind {
            NodeKind::Video(v) => out.push(((node.id, v.blob), v.autoplay, v.looped, !v.muted)),
            NodeKind::Group(children) => videos(children, out),
            _ => {}
        }
    }
}

impl VideoClock {
    /// Moves to `scene` at `now`: videos also in the old scene keep their
    /// start time, new autoplay videos start now, everything else stops.
    /// A new video whose file another output is already playing joins it
    /// (`playing` maps a blob to that start time).
    pub fn show(&mut self, scene: &Scene, now: f64) {
        self.show_joining(scene, now, &HashMap::new());
    }

    pub fn show_joining(&mut self, scene: &Scene, now: f64, playing: &HashMap<BlobHash, f64>) {
        let mut found = Vec::new();
        videos(&scene.nodes, &mut found);
        let mut next = HashMap::new();
        let mut looped = HashSet::new();
        let mut sounding = HashSet::new();
        for (key, autoplay, loops, audible) in found {
            let start = match self.started.get(&key) {
                Some(&start) => start,
                None if autoplay => playing.get(&key.1).copied().unwrap_or(now),
                None => continue,
            };
            next.insert(key, start);
            if loops {
                looped.insert(key);
            }
            if audible {
                sounding.insert(key);
            }
        }
        self.started = next;
        self.looped = looped;
        self.sounding = sounding;
    }

    /// A build step's `Effect::Play` on `node`: starts that video (one that
    /// does not autoplay) at `now`. Already playing, it carries on.
    pub fn play(&mut self, scene: &Scene, node: NodeId, now: f64) {
        let mut found = Vec::new();
        videos(&scene.nodes, &mut found);
        for (key, _, loops, audible) in found {
            if key.0 == node && !self.started.contains_key(&key) {
                self.started.insert(key, now);
                if loops {
                    self.looped.insert(key);
                }
                if audible {
                    self.sounding.insert(key);
                }
            }
        }
    }

    /// The players this output needs.
    pub(crate) fn players(&self) -> impl Iterator<Item = (VideoKey, PlayerKey)> + '_ {
        self.started.iter().map(|(key, &start)| {
            (
                *key,
                PlayerKey::new(key.1, start, self.looped.contains(key)),
            )
        })
    }
}

/// A video frame ready to draw.
#[derive(Clone)]
pub(crate) struct FrameView {
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
}

/// One decoder: the file, when its first output started it, and whether
/// it loops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PlayerKey {
    blob: BlobHash,
    start: u64,
    looped: bool,
}

impl PlayerKey {
    pub fn new(blob: BlobHash, start: f64, looped: bool) -> Self {
        Self {
            blob,
            start: start.to_bits(),
            looped,
        }
    }
}

enum Player {
    /// Reading the file and starting the decoder, on its own thread.
    Opening(Receiver<Result<Video, MediaError>>),
    Playing(Box<Playing>),
    Failed,
}

struct Playing {
    video: Video,
    /// When the first frame went up (render-thread ms); playback time
    /// counts from here, so a slow open never skips the start.
    origin: Option<f64>,
    /// Presentation time of the frame in `texture`.
    shown: Option<f64>,
    texture: Option<FrameView>,
    finished: bool,
    /// The file's sound, while an audible output shows this video.
    sound: Option<Sound>,
}

#[derive(Default)]
pub(crate) struct Players {
    players: HashMap<PlayerKey, Player>,
}

impl Players {
    /// Keeps the players in `wanted`, opens new ones and drops the rest.
    pub fn sync(&mut self, wanted: &HashSet<PlayerKey>, blobs: &Arc<dyn BlobSource>) {
        self.players.retain(|key, _| wanted.contains(key));
        for key in wanted {
            if self.players.contains_key(key) {
                continue;
            }
            let Some(path) = blobs.path(&key.blob) else {
                eprintln!(
                    "lampstand-render: video {:?} is not in the library",
                    key.blob
                );
                self.players.insert(*key, Player::Failed);
                continue;
            };
            let (tx, rx) = std::sync::mpsc::channel();
            let looped = key.looped;
            let spawned = std::thread::Builder::new()
                .name("lampstand-video-open".into())
                .spawn(move || {
                    let _ = tx.send(Video::open(&path, looped));
                });
            let player = match spawned {
                Ok(_) => Player::Opening(rx),
                Err(_) => Player::Failed,
            };
            self.players.insert(*key, player);
        }
    }

    /// Moves every player to `now` (render-thread ms), uploading frames
    /// that changed. Returns the players whose picture changed.
    pub fn advance(&mut self, now: f64, gpu: &Gpu) -> HashSet<PlayerKey> {
        let mut changed = HashSet::new();
        for (key, player) in &mut self.players {
            if let Player::Opening(rx) = player {
                *player = match rx.try_recv() {
                    Ok(Ok(video)) => Player::Playing(Box::new(Playing {
                        video,
                        origin: None,
                        shown: None,
                        texture: None,
                        finished: false,
                        sound: None,
                    })),
                    Ok(Err(e)) => {
                        eprintln!("lampstand-render: video {:?}: {e}", key.blob);
                        Player::Failed
                    }
                    Err(TryRecvError::Empty) => continue,
                    Err(TryRecvError::Disconnected) => Player::Failed,
                };
            }
            let Player::Playing(p) = player else {
                continue;
            };
            let Playing {
                video,
                origin,
                shown,
                texture,
                finished,
                ..
            } = &mut **p;
            if *finished {
                continue;
            }
            let elapsed = origin.map_or(0.0, |o| (now - o) / 1000.0);
            let Some(frame) = video.frame_at(elapsed) else {
                continue;
            };
            origin.get_or_insert(now);
            if *shown != Some(frame.time) {
                upload(gpu, texture, &frame);
                *shown = Some(frame.time);
                changed.insert(*key);
            }
            *finished = video.finished(elapsed);
        }
        changed
    }

    /// Plays the sound of the players in `audible` and silences the rest.
    /// A sound starts once its video has its first frame up, at the point
    /// the picture has reached.
    pub fn sync_sound(
        &mut self,
        audible: &HashSet<PlayerKey>,
        blobs: &Arc<dyn BlobSource>,
        now: f64,
    ) {
        for (key, player) in &mut self.players {
            let Player::Playing(p) = player else {
                continue;
            };
            if !audible.contains(key) || p.finished {
                p.sound = None;
            } else if p.sound.is_none() {
                let (Some(origin), Some(path)) = (p.origin, blobs.path(&key.blob)) else {
                    continue;
                };
                let elapsed = ((now - origin) / 1000.0).max(0.0);
                let at = lampstand_media::media_time(elapsed, p.video.duration(), key.looped);
                p.sound = Some(Sound::start(path, at, key.looped));
            }
        }
    }

    pub fn frame(&self, key: &PlayerKey) -> Option<&FrameView> {
        match self.players.get(key)? {
            Player::Playing(p) => p.texture.as_ref(),
            _ => None,
        }
    }

    /// Opening, or playing and not yet at its end: the output showing it
    /// needs a frame every refresh.
    pub fn running(&self, key: &PlayerKey) -> bool {
        match self.players.get(key) {
            Some(Player::Opening(_)) => true,
            Some(Player::Playing(p)) => !p.finished,
            _ => false,
        }
    }
}

fn upload(gpu: &Gpu, texture: &mut Option<FrameView>, frame: &Frame) {
    let size = (frame.width, frame.height);
    let max = gpu.max_texture_size();
    if size.0 == 0 || size.1 == 0 || size.0 > max || size.1 > max {
        return;
    }
    if texture.as_ref().is_none_or(|t| (t.width, t.height) != size) {
        // The view keeps its texture alive.
        let t = Texture::new(&gpu.device, "video frame", size.0, size.1, 1, 1);
        *texture = Some(FrameView {
            view: t.view,
            width: size.0,
            height: size.1,
        });
    }
    let Some(view) = texture else {
        return;
    };
    gpu.queue.write_texture(
        view.view.texture().as_image_copy(),
        &frame.rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * size.0),
            rows_per_image: Some(size.1),
        },
        wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
    );
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
    fn a_new_video_joins_another_output_playing_the_same_file() {
        let mut stage = VideoClock::default();
        let playing = HashMap::from([(BlobHash::of(b"waves"), 100.0)]);
        stage.show_joining(&scene(vec![video(7, b"waves", true)]), 900.0, &playing);
        assert_eq!(
            stage.started.get(&(7, BlobHash::of(b"waves"))),
            Some(&100.0)
        );
        assert!(stage.looped.contains(&(7, BlobHash::of(b"waves"))));
        // Already playing here: keeps its own start.
        let later = HashMap::from([(BlobHash::of(b"waves"), 500.0)]);
        stage.show_joining(&scene(vec![video(7, b"waves", true)]), 1000.0, &later);
        assert_eq!(
            stage.started.get(&(7, BlobHash::of(b"waves"))),
            Some(&100.0)
        );
    }

    #[test]
    fn videos_without_autoplay_wait_for_a_play_step() {
        let mut clock = VideoClock::default();
        let s = scene(vec![video(1, b"clip", false)]);
        clock.show(&s, 0.0);
        assert!(clock.started.is_empty());
        clock.play(&s, 1, 50.0);
        clock.play(&s, 1, 80.0);
        assert_eq!(clock.started.get(&(1, BlobHash::of(b"clip"))), Some(&50.0));
    }
}
