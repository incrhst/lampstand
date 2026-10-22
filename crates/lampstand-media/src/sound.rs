//! The sound of a video file on the default output device.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use deckcraft_media::Pcm;

/// A file's audio track playing. Dropping it stops the sound.
///
/// The audio is decoded and the device opened on a thread of its own (a
/// cpal stream must stay on the thread that made it), so `start` never
/// waits. Playback begins `from` seconds in, counted from the call, so a
/// slow decode does not put the sound behind the picture.
pub struct Sound {
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Sound {
    /// Plays the audio of the file at `path`, looping if `looped`. A file
    /// with no audio, or no output device, is silent.
    pub fn start(path: PathBuf, from: f64, looped: bool) -> Sound {
        let (stop, stopped) = channel::<()>();
        let began = std::time::Instant::now();
        let thread = std::thread::Builder::new()
            .name("lampstand-sound".into())
            .spawn(move || {
                let Some(pcm) = decode(&path) else {
                    return;
                };
                let Some(_stream) = open(Arc::new(pcm), from, began, looped) else {
                    return;
                };
                // Holds the stream open until `Sound` drops its sender.
                let _ = stopped.recv();
            })
            .ok();
        Sound {
            stop: Some(stop),
            thread,
        }
    }
}

impl Drop for Sound {
    fn drop(&mut self) {
        drop(self.stop.take());
        // Not joined: the thread may still be decoding, and the render
        // thread must not wait for it.
        drop(self.thread.take());
    }
}

fn decode(path: &std::path::Path) -> Option<Pcm> {
    let bytes = Arc::new(std::fs::read(path).ok()?);
    match deckcraft_media::audio::decode(&bytes) {
        Ok(pcm) if pcm.frames() > 0 => Some(pcm),
        Ok(_) => None,
        Err(e) => {
            eprintln!("lampstand-media: no sound from {}: {e}", path.display());
            None
        }
    }
}

fn open(pcm: Arc<Pcm>, from: f64, began: std::time::Instant, looped: bool) -> Option<cpal::Stream> {
    let device = cpal::default_host().default_output_device()?;
    let config = device.default_output_config().ok()?;
    let format = config.sample_format();
    let config: cpal::StreamConfig = config.into();
    // Where playback is now, in the file's own sample frames.
    let start = ((from + began.elapsed().as_secs_f64()) * f64::from(pcm.rate)).max(0.0);
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, &config, pcm, start, looped),
        SampleFormat::I16 => build::<i16>(&device, &config, pcm, start, looped),
        SampleFormat::U16 => build::<u16>(&device, &config, pcm, start, looped),
        other => {
            eprintln!("lampstand-media: unsupported sample format {other:?}");
            return None;
        }
    }?;
    stream.play().ok()?;
    Some(stream)
}

fn build<T: SizedSample + FromSample<f32> + Send + 'static>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    pcm: Arc<Pcm>,
    start: f64,
    looped: bool,
) -> Option<cpal::Stream> {
    let channels = usize::from(config.channels.max(1));
    let step = f64::from(pcm.rate) / f64::from(config.sample_rate.0.max(1));
    let total = pcm.frames() as f64;
    let mut position = start;
    device
        .build_output_stream(
            config,
            move |out: &mut [T], _| {
                for frame in out.chunks_mut(channels) {
                    if looped && position >= total {
                        position %= total;
                    }
                    let live = position < total;
                    for (c, slot) in frame.iter_mut().enumerate() {
                        *slot = T::from_sample(if live { sample(&pcm, position, c, channels) } else { 0.0 });
                    }
                    position += step;
                }
            },
            |e| eprintln!("lampstand-media: audio output: {e}"),
            None,
        )
        .ok()
}

/// Channel `c` of the output at fractional file frame `at`: linear between
/// neighbouring frames. A mono file plays on every channel; extra source
/// channels beyond the output's are dropped.
fn sample(pcm: &Pcm, at: f64, c: usize, out_channels: usize) -> f32 {
    let i = at as usize;
    let t = (at - i as f64) as f32;
    let ch = if pcm.channels == 1 {
        0
    } else if out_channels == 1 {
        // Folds stereo down.
        return (pcm.at(i, 0) + pcm.at(i, 1)) * 0.5 * (1.0 - t) + (pcm.at(i + 1, 0) + pcm.at(i + 1, 1)) * 0.5 * t;
    } else {
        c
    };
    pcm.at(i, ch) * (1.0 - t) + pcm.at(i + 1, ch) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo() -> Pcm {
        Pcm {
            rate: 10,
            channels: 2,
            samples: vec![0.0, 1.0, 1.0, 0.0, 0.0, 1.0],
        }
    }

    #[test]
    fn samples_blend_between_frames_and_keep_channels_apart() {
        let pcm = stereo();
        assert_eq!(sample(&pcm, 0.0, 0, 2), 0.0);
        assert_eq!(sample(&pcm, 0.0, 1, 2), 1.0);
        assert_eq!(sample(&pcm, 0.5, 0, 2), 0.5);
    }

    #[test]
    fn mono_plays_on_every_channel_and_stereo_folds_to_mono() {
        let mono = Pcm {
            rate: 10,
            channels: 1,
            samples: vec![0.25, 0.25],
        };
        assert_eq!(sample(&mono, 0.0, 1, 2), 0.25);
        assert_eq!(sample(&stereo(), 0.0, 0, 1), 0.5);
    }

    #[test]
    fn a_file_without_audio_makes_no_sound_and_does_not_panic() {
        let s = Sound::start(PathBuf::from("/no/such/file.mp4"), 0.0, false);
        drop(s);
    }
}
