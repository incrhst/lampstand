//! Plays the bundled clip (2 s, 160×90, 30 fps H.264, made with
//! `ffmpeg -f lavfi -i testsrc2=size=160x90:rate=30 -t 2 -c:v libx264
//! -pix_fmt yuv420p`).

use std::path::PathBuf;
use std::time::Duration;

use lampstand_media::{poster, Video};

fn clip() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/clip-2s.mp4")
}

const WAIT: Duration = Duration::from_secs(5);

#[test]
fn plays_the_clip_to_the_end_and_counts_frames() {
    let mut video = Video::open(&clip(), false).unwrap();
    assert_eq!(video.size(), (160, 90));
    assert!(
        (video.duration() - 2.0).abs() < 0.05,
        "{}",
        video.duration()
    );

    let mut times = Vec::new();
    let mut elapsed = 0.0;
    while elapsed < 2.5 {
        let frame = video.frame_at_blocking(elapsed, WAIT).unwrap();
        assert_eq!((frame.width, frame.height), (160, 90));
        assert_eq!(frame.rgba.len(), 160 * 90 * 4);
        if times.last() != Some(&frame.time) {
            times.push(frame.time);
        }
        elapsed += 1.0 / 60.0;
    }
    assert_eq!(times.len(), 60, "{times:?}");
    assert!(times.windows(2).all(|w| w[0] < w[1]));
    // Held on the last frame once the clip is over.
    assert!(video.finished(3.0));
    let last = video.frame_at(10.0).unwrap();
    assert!((last.time - 59.0 / 30.0).abs() < 1e-3, "{}", last.time);
}

#[test]
fn a_looped_clip_starts_again() {
    let mut video = Video::open(&clip(), true).unwrap();
    let late = video.frame_at_blocking(1.9, WAIT).unwrap();
    assert!(late.time > 1.8);
    let wrapped = video.frame_at_blocking(2.5, WAIT).unwrap();
    assert!((wrapped.time - 0.5).abs() < 0.04, "{}", wrapped.time);
    assert!(!video.finished(100.0));
}

#[test]
fn poster_is_the_first_frame() {
    let frame = poster(&clip()).unwrap();
    assert_eq!((frame.width, frame.height, frame.time), (160, 90, 0.0));
}

#[test]
fn missing_and_non_video_files_are_errors() {
    assert!(Video::open(&clip().with_file_name("nope.mp4"), false).is_err());
    let not_video = Video::from_bytes(std::sync::Arc::new(b"hello".to_vec()), false);
    assert!(not_video.is_err());
}
