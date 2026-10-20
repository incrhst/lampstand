//! Video playback for backgrounds, media items and video embedded in
//! decks.
//!
//! PRD: LV-4, PS-3. NDI, Spout and Syphon (OB-3) are Phase 2.
//!
//! Decoding is DeckCraft's (`deckcraft-media`, T27): our own pure-Rust
//! H.264, HEVC, VP9 and AV1 decoders for MP4/MOV and WebM/Matroska, to RGBA
//! frames. So nothing has to be installed beside the app, on any platform.
//! On an M1 one core decodes 1080p H.264 at 53 to 66 fps and 1080p HEVC at
//! 74 fps (colour conversion included), about twice real time for 30 fps
//! video. 1080p60 is at the limit, and 4K is not real time. There is no
//! hardware decoding; if field reports show older machines dropping frames,
//! a VideoToolbox / Media Foundation backend can sit behind `Video`.
//!
//! Playback follows the caller's clock. Each `Video` decodes a few frames
//! ahead on its own worker thread; the render thread asks `frame_at` for
//! the frame due `elapsed` seconds after the video started, once per
//! display frame, and never waits. Looping and seeking are a function of
//! that time (`media_time`).
//!
//! Not yet: audio. Videos play silently whatever `VideoNode::muted` says.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use deckcraft_media::VideoFeed;

/// A decoded picture: RGBA8, opaque, with its presentation time in seconds.
pub use deckcraft_media::Frame;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaError {
    /// A format or codec we cannot decode (WMV, ProRes, a file with no
    /// video track).
    Unsupported(String),
    /// The file is missing, unreadable or corrupt.
    Unreadable(String),
}

impl std::fmt::Display for MediaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MediaError::Unsupported(what) => write!(f, "{what} can't be played"),
            MediaError::Unreadable(why) => write!(f, "the video can't be read: {why}"),
        }
    }
}

impl std::error::Error for MediaError {}

impl From<deckcraft_media::MediaError> for MediaError {
    fn from(e: deckcraft_media::MediaError) -> Self {
        match e {
            deckcraft_media::MediaError::Unsupported(what) => MediaError::Unsupported(what),
            other => MediaError::Unreadable(other.to_string()),
        }
    }
}

/// Where in the media `elapsed` seconds of playback lands: wrapped for a
/// looped video, held at the end otherwise.
pub fn media_time(elapsed: f64, duration: f64, looped: bool) -> f64 {
    let elapsed = elapsed.max(0.0);
    if duration <= 0.0 {
        elapsed
    } else if looped {
        elapsed % duration
    } else {
        elapsed.min(duration)
    }
}

/// One playing video file.
pub struct Video {
    feed: VideoFeed,
    size: (u32, u32),
    duration: f64,
    looped: bool,
}

impl Video {
    /// Reads and opens `path`. This reads the whole file, so call it off
    /// the render thread.
    pub fn open(path: &Path, looped: bool) -> Result<Video, MediaError> {
        let bytes = std::fs::read(path).map_err(|e| MediaError::Unreadable(e.to_string()))?;
        Self::from_bytes(Arc::new(bytes), looped)
    }

    pub fn from_bytes(bytes: Arc<Vec<u8>>, looped: bool) -> Result<Video, MediaError> {
        let info = deckcraft_media::probe(&bytes)?;
        let Some(video) = &info.video else {
            return Err(MediaError::Unsupported("a file without video".into()));
        };
        if !video.decodable {
            return Err(MediaError::Unsupported(format!("{} video", video.codec)));
        }
        let counted = if video.fps > 0.0 {
            video.frames as f64 / video.fps
        } else {
            0.0
        };
        let duration = info.duration().max(counted);
        let feed = VideoFeed::new(bytes)?;
        Ok(Video {
            size: feed.size(),
            feed,
            duration,
            looped,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Seconds; 0 when the file does not say.
    pub fn duration(&self) -> f64 {
        self.duration
    }

    pub fn looped(&self) -> bool {
        self.looped
    }

    /// The frame on screen `elapsed` seconds after playback started, or
    /// `None` until the first frame has decoded. Never blocks.
    pub fn frame_at(&mut self, elapsed: f64) -> Option<Arc<Frame>> {
        let t = media_time(elapsed, self.duration, self.looped);
        self.feed.frame_at(t)
    }

    /// `frame_at`, waiting up to `timeout` for the frame to decode (tests,
    /// offline rendering).
    pub fn frame_at_blocking(&mut self, elapsed: f64, timeout: Duration) -> Option<Arc<Frame>> {
        let t = media_time(elapsed, self.duration, self.looped);
        self.feed.frame_at_blocking(t, timeout)
    }

    /// The last frame is on screen and nothing will change again. Never
    /// true for a looped video.
    pub fn finished(&self, elapsed: f64) -> bool {
        (!self.looped || self.duration <= 0.0) && elapsed >= self.duration && self.feed.ended()
    }
}

/// The first frame of the video at `path`, for thumbnails.
pub fn poster(path: &Path) -> Result<Frame, MediaError> {
    let bytes = std::fs::read(path).map_err(|e| MediaError::Unreadable(e.to_string()))?;
    Ok(deckcraft_media::poster_frame(&Arc::new(bytes), 0.0)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looped_time_wraps_and_unlooped_time_holds_at_the_end() {
        assert_eq!(media_time(2.5, 2.0, true), 0.5);
        assert_eq!(media_time(2.5, 2.0, false), 2.0);
        assert_eq!(media_time(1.0, 2.0, true), 1.0);
        assert_eq!(media_time(-1.0, 2.0, true), 0.0);
        // Unknown duration: plain wall time.
        assert_eq!(media_time(5.0, 0.0, true), 5.0);
    }
}
