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
//! Sound is separate: `Sound` decodes a file's audio track and plays it on
//! the default output device from a given point. The caller starts it only
//! where it should be heard.

mod sound;
pub use sound::Sound;

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

/// The first frame of the video at `path`.
pub fn poster(path: &Path) -> Result<Frame, MediaError> {
    let bytes = std::fs::read(path).map_err(|e| MediaError::Unreadable(e.to_string()))?;
    Ok(deckcraft_media::poster_frame(&Arc::new(bytes), 0.0)?)
}

/// Mean and spread of the brightness (0 to 255) of a picture, from a
/// sample of its pixels.
fn brightness(frame: &Frame) -> (f32, f32) {
    let pixels = frame.rgba.len() / 4;
    if pixels == 0 {
        return (0.0, 0.0);
    }
    let step = (pixels / 4096).max(1);
    let (mut sum, mut sum_sq, mut n) = (0.0f64, 0.0f64, 0.0f64);
    for px in frame.rgba.as_chunks::<4>().0.iter().step_by(step) {
        let y = 0.2126 * f64::from(px[0]) + 0.7152 * f64::from(px[1]) + 0.0722 * f64::from(px[2]);
        sum += y;
        sum_sq += y * y;
        n += 1.0;
    }
    let mean = sum / n;
    let spread = (sum_sq / n - mean * mean).max(0.0).sqrt();
    (mean as f32, spread as f32)
}

/// A picture with nothing to preview: one flat colour, or all but black or
/// white. Typical of the first frame of a video that fades in from black.
pub fn is_blank(frame: &Frame) -> bool {
    let (mean, spread) = brightness(frame);
    spread < 6.0 || (mean < 14.0 && spread < 12.0) || (mean > 245.0 && spread < 12.0)
}

/// The first picture that is not blank, or the one with the most going on
/// if every one is.
fn choose(frames: impl IntoIterator<Item = Frame>) -> Option<Frame> {
    let mut best: Option<(f32, Frame)> = None;
    for frame in frames {
        if !is_blank(&frame) {
            return Some(frame);
        }
        let spread = brightness(&frame).1;
        if best.as_ref().is_none_or(|(s, _)| spread > *s) {
            best = Some((spread, frame));
        }
    }
    best.map(|(_, frame)| frame)
}

/// Where in a video of `duration` seconds to look for a picture: the start,
/// then points through it. A duration the file does not give tries the first
/// few seconds.
pub fn poster_times(duration: f64) -> Vec<f64> {
    if duration <= 0.0 || !duration.is_finite() {
        return vec![0.0, 1.0, 3.0, 6.0];
    }
    let last = (duration - 0.1).max(0.0);
    let mut times = vec![0.0];
    for fraction in [0.1, 0.25, 0.5, 0.75] {
        let t = (duration * fraction).min(last);
        if t > times[times.len() - 1] + 0.01 {
            times.push(t);
        }
    }
    times
}

/// A picture of the video at `path` worth showing as its thumbnail: the
/// first frame, unless that is blank (a fade-in from black, a title card
/// that has not appeared yet), in which case the first of several later
/// points that is not. If every point is blank, the one with most in it.
pub fn poster_best(path: &Path) -> Result<Frame, MediaError> {
    let bytes = Arc::new(std::fs::read(path).map_err(|e| MediaError::Unreadable(e.to_string()))?);
    let duration = deckcraft_media::probe(&bytes)
        .map(|info| info.duration())
        .unwrap_or(0.0);
    let mut first_error = None;
    let frames = poster_times(duration).into_iter().filter_map(|t| {
        match deckcraft_media::poster_frame(&bytes, t) {
            Ok(frame) => Some(frame),
            Err(e) => {
                first_error.get_or_insert(e);
                None
            }
        }
    });
    match choose(frames) {
        Some(frame) => Ok(frame),
        None => Err(first_error.map_or(
            MediaError::Unsupported("a video with no picture".into()),
            MediaError::from,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Frame {
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&pixel(x, y));
            }
        }
        Frame {
            time: 0.0,
            width,
            height,
            rgba,
        }
    }

    fn flat(value: u8) -> Frame {
        frame(32, 18, |_, _| [value, value, value, 255])
    }

    fn busy() -> Frame {
        frame(32, 18, |x, y| {
            let v = ((x * 8 + y * 5) % 256) as u8;
            [v, 255 - v, v / 2, 255]
        })
    }

    #[test]
    fn black_white_and_flat_pictures_are_blank_and_a_busy_one_is_not() {
        assert!(is_blank(&flat(0)));
        assert!(is_blank(&flat(255)));
        assert!(is_blank(&flat(128)), "a flat grey has nothing in it either");
        assert!(!is_blank(&busy()));
    }

    #[test]
    fn a_black_picture_with_a_small_bright_logo_is_not_blank() {
        let logo = frame(64, 36, |x, y| {
            if (20..44).contains(&x) && (12..24).contains(&y) {
                [255; 4]
            } else {
                [0, 0, 0, 255]
            }
        });
        assert!(!is_blank(&logo));
    }

    #[test]
    fn the_first_picture_that_is_not_blank_is_chosen() {
        let chosen = choose([flat(0), flat(0), busy(), flat(255)]).unwrap();
        assert_eq!(chosen.rgba, busy().rgba);
    }

    #[test]
    fn when_every_picture_is_blank_the_one_with_most_in_it_is_chosen() {
        // Faint noise on black is still blank, but has more going on than flat black.
        let faint = frame(32, 18, |x, _| {
            let v = if x % 2 == 0 { 9 } else { 0 };
            [v, v, v, 255]
        });
        let chosen = choose([flat(0), faint.clone(), flat(0)]).unwrap();
        assert_eq!(chosen.rgba, faint.rgba);
        assert!(choose(Vec::new()).is_none());
    }

    #[test]
    fn poster_times_start_at_the_start_and_run_through_the_video() {
        assert_eq!(poster_times(0.0), [0.0, 1.0, 3.0, 6.0]);
        let t = poster_times(100.0);
        assert_eq!(t, [0.0, 10.0, 25.0, 50.0, 75.0]);
        // A clip of two seconds stays inside it.
        let short = poster_times(2.0);
        assert!(short.iter().all(|t| *t < 2.0), "{short:?}");
        assert!(short.windows(2).all(|w| w[0] < w[1]), "{short:?}");
    }

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
