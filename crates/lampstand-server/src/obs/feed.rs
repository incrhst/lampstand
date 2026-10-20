//! One feed per output with a connected browser source: it reads the
//! backend's blocking scene channel once, numbers the scenes, and lets any
//! number of pages follow (and ask for a deck frame of the current scene).

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use lampstand_core::output::OutputId;
use lampstand_core::scene::{NodeKind, Scene, COMPOSITOR_NODE_BASE};
use lampstand_render::Renderer;
use tokio::sync::{watch, Mutex as AsyncMutex};

use crate::Backend;

#[derive(Clone)]
pub(crate) struct Current {
    /// 0 until the first scene; then counts up, across feeds: a number is
    /// never reused while the server runs, so a frame URL always means the
    /// same scene.
    pub seq: u64,
    pub scene: Option<Arc<Scene>>,
    /// The backend ended the output's scene channel (its URL was removed):
    /// pages following this feed disconnect and the feed is not reused.
    pub closed: bool,
}

pub(crate) struct Feed {
    tx: watch::Sender<Current>,
    /// The last deck frame asked for: (seq, WebP bytes).
    frame: AsyncMutex<Option<(u64, Arc<Vec<u8>>)>>,
}

#[derive(Default)]
pub(crate) struct Feeds {
    feeds: Mutex<HashMap<OutputId, Weak<Feed>>>,
    seq: Arc<AtomicU64>,
}

impl Feeds {
    /// The feed for `output`, started if no page is following it yet.
    /// `None` when the backend has no scenes for the output.
    pub fn subscribe(&self, backend: &dyn Backend, output: OutputId) -> Option<Arc<Feed>> {
        let mut feeds = self.feeds.lock().unwrap();
        let running = feeds
            .get(&output)
            .and_then(Weak::upgrade)
            .filter(|feed| !feed.tx.borrow().closed);
        if running.is_some() {
            return running;
        }
        let rx = backend.watch_scene(output)?;
        let (tx, _) = watch::channel(Current {
            seq: 0,
            scene: None,
            closed: false,
        });
        let feed = Arc::new(Feed {
            tx,
            frame: AsyncMutex::new(None),
        });
        let weak = Arc::downgrade(&feed);
        let counter = self.seq.clone();
        feeds.insert(output, weak.clone());
        std::thread::Builder::new()
            .name("lampstand-obs-feed".into())
            .spawn(move || pump(&rx, &weak, &counter))
            .ok()?;
        Some(feed)
    }

    /// The running feed for `output`, for frame requests.
    pub fn get(&self, output: OutputId) -> Option<Arc<Feed>> {
        self.feeds.lock().unwrap().get(&output)?.upgrade()
    }
}

/// Moves scenes from the blocking channel into the feed until nobody
/// follows it or the backend drops the channel.
fn pump(rx: &std::sync::mpsc::Receiver<Arc<Scene>>, feed: &Weak<Feed>, seq: &AtomicU64) {
    loop {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(scene) => {
                let Some(feed) = feed.upgrade() else { return };
                feed.tx.send_modify(|cur| {
                    cur.seq = seq.fetch_add(1, Ordering::Relaxed) + 1;
                    cur.scene = Some(scene);
                });
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if feed.strong_count() == 0 {
                    return;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                if let Some(feed) = feed.upgrade() {
                    feed.tx.send_modify(|cur| cur.closed = true);
                }
                return;
            }
        }
    }
}

impl Feed {
    pub fn watch(&self) -> watch::Receiver<Current> {
        self.tx.subscribe()
    }

    /// Scene number `seq` rasterised as a lossless WebP, or `None` if it is
    /// no longer the current scene or cannot be drawn. Renders once per
    /// scene however many pages ask.
    pub async fn frame(&self, renderer: Arc<Renderer>, seq: u64) -> Option<Arc<Vec<u8>>> {
        let mut cached = self.frame.lock().await;
        let current = self.tx.borrow().clone();
        if current.seq != seq {
            return None;
        }
        if let Some((n, bytes)) = &*cached {
            if *n == seq {
                return Some(bytes.clone());
            }
        }
        let scene = current.scene?;
        let bytes = tokio::task::spawn_blocking(move || rasterise(&renderer, &scene))
            .await
            .ok()??;
        let bytes = Arc::new(bytes);
        *cached = Some((seq, bytes.clone()));
        Some(bytes)
    }
}

fn rasterise(renderer: &Renderer, scene: &Scene) -> Option<Vec<u8>> {
    let image = renderer
        .render_offscreen(
            scene,
            scene.width.round().max(1.0) as u32,
            scene.height.round().max(1.0) as u32,
        )
        .ok()?;
    let mut out = Cursor::new(Vec::new());
    image::codecs::webp::WebPEncoder::new_lossless(&mut out)
        .encode(
            &image.pixels,
            image.width,
            image.height,
            image::ExtendedColorType::Rgba8,
        )
        .ok()?;
    Some(out.into_inner())
}

/// Whether the scene is one deck slide: the compositor's group around
/// content nodes, and nothing else. Browser sources get those as frames
/// rasterised by our renderer (PRD OB-2) instead of redrawing PPTX shapes.
pub(crate) fn is_deck(scene: &Scene) -> bool {
    match scene.nodes.as_slice() {
        [group] => match &group.kind {
            NodeKind::Group(children) => {
                group.id >= COMPOSITOR_NODE_BASE
                    && children.iter().all(|c| c.id < COMPOSITOR_NODE_BASE)
            }
            _ => false,
        },
        _ => false,
    }
}
