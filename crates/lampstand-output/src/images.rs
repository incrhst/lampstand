//! Deck images and the logo, fetched over HTTP from the server and kept
//! decoded in a small LRU.
//!
//! The renderer keeps a GPU texture only while its image is on screen, so
//! the CPU copy here is what makes going live fast: a prefetched image needs
//! one texture upload and no fetch or PNG decode.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use lampstand_render::Image;

/// An image at the width one window needs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageKey {
    /// Server-relative, as in the snapshot.
    pub url: String,
    /// Window width in pixels, sent as `?w=`.
    pub width: u32,
}

impl ImageKey {
    /// The full URL to fetch, given the server's `http://host:port`.
    pub fn request_url(&self, http_base: &str) -> String {
        let sep = if self.url.contains('?') { '&' } else { '?' };
        let url = if self.url.starts_with("http://") || self.url.starts_with("https://") {
            self.url.clone()
        } else if self.url.starts_with('/') {
            format!("{http_base}{}", self.url)
        } else {
            format!("{http_base}/{}", self.url)
        };
        format!("{url}{sep}w={}", self.width)
    }
}

/// GET `url` and decode it to RGBA8. Blocking; run it off the window thread.
pub fn fetch(url: &str) -> Result<Image, String> {
    let agent = ureq::Agent::new_with_config(ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(10))).build());
    let mut response = agent.get(url).call().map_err(|e| format!("{url}: {e}"))?;
    let bytes = response.body_mut().with_config().limit(256 << 20).read_to_vec().map_err(|e| format!("{url}: {e}"))?;
    let decoded = image::load_from_memory(&bytes).map_err(|e| format!("{url}: {e}"))?;
    Ok(decoded.into_rgba8().into())
}

/// Decoded images by key, least recently used dropped first, plus which keys
/// are being fetched and which failed.
#[derive(Debug)]
pub struct ImageCache {
    capacity: usize,
    entries: VecDeque<(ImageKey, Arc<Image>)>,
    pending: HashSet<ImageKey>,
    failed: HashSet<ImageKey>,
}

/// What the cache knows about a key.
#[derive(Debug, Clone, PartialEq)]
pub enum Lookup {
    Ready(Arc<Image>),
    Pending,
    Failed,
    /// Not asked for yet.
    Missing,
}

impl ImageCache {
    pub fn new(capacity: usize) -> Self {
        ImageCache { capacity: capacity.max(1), entries: VecDeque::new(), pending: HashSet::new(), failed: HashSet::new() }
    }

    /// Look `key` up, marking it most recently used.
    pub fn get(&mut self, key: &ImageKey) -> Lookup {
        if let Some(i) = self.entries.iter().position(|(k, _)| k == key) {
            let entry = self.entries.remove(i).expect("index in range");
            let image = entry.1.clone();
            self.entries.push_back(entry);
            Lookup::Ready(image)
        } else if self.pending.contains(key) {
            Lookup::Pending
        } else if self.failed.contains(key) {
            Lookup::Failed
        } else {
            Lookup::Missing
        }
    }

    /// Mark `key` as being fetched. Returns false if it is already cached,
    /// pending or failed, so the caller should not fetch it.
    pub fn start(&mut self, key: &ImageKey) -> bool {
        if self.get(key) != Lookup::Missing {
            return false;
        }
        self.pending.insert(key.clone());
        true
    }

    /// Record a finished fetch.
    pub fn finish(&mut self, key: ImageKey, result: Result<Arc<Image>, String>) {
        self.pending.remove(&key);
        match result {
            Ok(image) => {
                self.entries.retain(|(k, _)| *k != key);
                self.entries.push_back((key, image));
                while self.entries.len() > self.capacity {
                    self.entries.pop_front();
                }
            }
            Err(_) => {
                self.failed.insert(key);
            }
        }
    }

    /// Let failed images be fetched again, e.g. after the state changes or
    /// the server comes back.
    pub fn retry_failed(&mut self) {
        self.failed.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(url: &str) -> ImageKey {
        ImageKey { url: url.into(), width: 1920 }
    }

    fn img() -> Arc<Image> {
        Arc::new(Image::new(1, 1, vec![0, 0, 0, 255]))
    }

    #[test]
    fn request_urls() {
        let base = "http://127.0.0.1:7420";
        assert_eq!(key("/media/deck/a/0/1.png").request_url(base), "http://127.0.0.1:7420/media/deck/a/0/1.png?w=1920");
        assert_eq!(key("/x.png?v=2").request_url(base), "http://127.0.0.1:7420/x.png?v=2&w=1920");
        assert_eq!(key("http://h/x.png").request_url(base), "http://h/x.png?w=1920");
    }

    #[test]
    fn lru_drops_the_least_recently_used() {
        let mut c = ImageCache::new(2);
        for k in ["a", "b"] {
            assert!(c.start(&key(k)));
            assert!(!c.start(&key(k)), "already pending");
            c.finish(key(k), Ok(img()));
        }
        assert!(matches!(c.get(&key("a")), Lookup::Ready(_)), "a is now most recent");
        c.start(&key("c"));
        c.finish(key("c"), Ok(img()));
        assert_eq!(c.len(), 2);
        assert_eq!(c.get(&key("b")), Lookup::Missing);
        assert!(matches!(c.get(&key("a")), Lookup::Ready(_)));
    }

    #[test]
    fn failures_are_retried_on_request() {
        let mut c = ImageCache::new(2);
        c.start(&key("a"));
        c.finish(key("a"), Err("404".into()));
        assert_eq!(c.get(&key("a")), Lookup::Failed);
        assert!(!c.start(&key("a")));
        c.retry_failed();
        assert!(c.start(&key("a")));
    }
}
