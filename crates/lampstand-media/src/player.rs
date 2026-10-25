//! The Audio holder's player: one piece of sound at a time (music, a
//! backing track, a video's sound) that can be paused, stopped with a fade,
//! or cut at once.
//!
//! [`Voice`] is the sound itself and has no device in it, so its rules are
//! tested without one. [`Player`] puts a voice on an output device chosen by
//! name. The device's stream stays on a thread of its own (a cpal stream
//! must stay on the thread that made it), so nothing here waits on audio
//! hardware.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use deckcraft_media::Pcm;

/// How long a pause, a resume and a panic stop ramp the volume, so none of
/// them clicks. Short enough that the panic stop is still instant.
const QUICK: f64 = 0.02;

/// What a voice does when its volume reaches the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Then {
    Nothing,
    Pause,
    Finish,
}

/// A piece of sound playing, paused, fading or finished.
pub struct Voice {
    pcm: Arc<Pcm>,
    /// Where it is, in the file's own sample frames.
    position: f64,
    paused: bool,
    finished: bool,
    gain: f32,
    target: f32,
    /// Gain change per output second while moving to `target`.
    slope: f32,
    then: Then,
}

impl Voice {
    /// Starts `pcm` at `from` seconds in, at full volume.
    pub fn new(pcm: Arc<Pcm>, from: f64) -> Voice {
        let position = (from.max(0.0) * f64::from(pcm.rate)).min(pcm.frames() as f64);
        Voice {
            pcm,
            position,
            paused: false,
            finished: false,
            gain: 1.0,
            target: 1.0,
            slope: 0.0,
            then: Then::Nothing,
        }
    }

    pub fn finished(&self) -> bool {
        self.finished
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Whether it is on its way out (a fade or a panic stop is running).
    pub fn ending(&self) -> bool {
        self.then == Then::Finish
    }

    pub fn position(&self) -> f64 {
        self.position / f64::from(self.pcm.rate.max(1))
    }

    pub fn length(&self) -> f64 {
        self.pcm.duration()
    }

    pub fn pause(&mut self) {
        if !self.paused && !self.finished {
            self.ramp(0.0, QUICK, Then::Pause);
        }
    }

    pub fn resume(&mut self) {
        if self.finished || self.then == Then::Finish {
            return;
        }
        self.paused = false;
        self.ramp(1.0, QUICK, Then::Nothing);
    }

    /// Fades out over `secs` and finishes.
    pub fn stop(&mut self, secs: f64) {
        if !self.finished {
            self.paused = false;
            self.ramp(0.0, secs.max(QUICK), Then::Finish);
        }
    }

    /// Silence now (a few milliseconds of ramp, no click) and finish.
    pub fn panic(&mut self) {
        if !self.finished {
            self.paused = false;
            self.ramp(0.0, 0.005, Then::Finish);
        }
    }

    fn ramp(&mut self, target: f32, secs: f64, then: Then) {
        // Nothing is audible (a paused voice): there is nothing to fade, and
        // a silent voice does not run its ramp.
        if then == Then::Finish && self.gain <= 0.0 {
            self.finished = true;
            return;
        }
        self.target = target;
        self.then = then;
        self.slope = ((target - self.gain).abs() as f64 / secs.max(0.001)) as f32;
    }

    /// Fills `out` (interleaved, `channels` wide) at `out_rate` Hz.
    pub fn render(&mut self, out: &mut [f32], channels: usize, out_rate: u32) {
        let channels = channels.max(1);
        let step = f64::from(self.pcm.rate) / f64::from(out_rate.max(1));
        let total = self.pcm.frames() as f64;
        let per_frame = self.slope / out_rate.max(1) as f32;
        for frame in out.chunks_mut(channels) {
            if self.finished || (self.paused && self.gain <= 0.0) {
                frame.fill(0.0);
                continue;
            }
            if self.position >= total {
                self.finished = true;
                frame.fill(0.0);
                continue;
            }
            for (c, slot) in frame.iter_mut().enumerate() {
                *slot = sample(&self.pcm, self.position, c, channels) * self.gain;
            }
            self.position += step;
            if self.gain != self.target {
                let toward = if self.target > self.gain {
                    per_frame
                } else {
                    -per_frame
                };
                self.gain += toward;
                if (toward > 0.0 && self.gain >= self.target)
                    || (toward < 0.0 && self.gain <= self.target)
                {
                    self.gain = self.target;
                }
                if self.gain == self.target {
                    match self.then {
                        Then::Pause => self.paused = true,
                        Then::Finish => self.finished = true,
                        Then::Nothing => {}
                    }
                }
            }
        }
    }
}

/// Channel `c` of the output at fractional file frame `at`: linear between
/// neighbouring frames. A mono file plays on every channel; a stereo file
/// folds down to one.
fn sample(pcm: &Pcm, at: f64, c: usize, out_channels: usize) -> f32 {
    let i = at as usize;
    let t = (at - i as f64) as f32;
    let ch = if pcm.channels == 1 {
        0
    } else if out_channels == 1 {
        return (pcm.at(i, 0) + pcm.at(i, 1)) * 0.5 * (1.0 - t)
            + (pcm.at(i + 1, 0) + pcm.at(i + 1, 1)) * 0.5 * t;
    } else {
        c
    };
    pcm.at(i, ch) * (1.0 - t) + pcm.at(i + 1, ch) * t
}

/// Decodes the audio of the file at `path`, or says why not.
pub fn load(path: &std::path::Path) -> Result<Arc<Pcm>, String> {
    let bytes = Arc::new(std::fs::read(path).map_err(|e| e.to_string())?);
    match deckcraft_media::audio::decode(&bytes) {
        Ok(pcm) if pcm.frames() > 0 => Ok(Arc::new(pcm)),
        Ok(_) => Err("the file has no sound".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// The names of the sound outputs on this computer, for the chooser.
pub fn output_devices() -> Vec<String> {
    let Ok(devices) = cpal::default_host().output_devices() else {
        return Vec::new();
    };
    let mut names: Vec<String> = devices.filter_map(|d| d.name().ok()).collect();
    names.sort();
    names.dedup();
    names
}

/// The name of the output used when none is chosen.
pub fn default_output() -> Option<String> {
    cpal::default_host().default_output_device()?.name().ok()
}

/// What the player is doing, for the screens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playing {
    pub paused: bool,
    pub ending: bool,
    pub position: f64,
    pub length: f64,
}

/// A voice on an output device. Dropping the player stops the sound.
pub struct Player {
    voice: Arc<Mutex<Option<Voice>>>,
    open: Arc<AtomicBool>,
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Player {
    /// Opens the output called `device` (the default output if `None`, or if
    /// no output has that name). Never waits: [`Player::output_open`] says
    /// whether the output came up.
    pub fn start(device: Option<String>) -> Player {
        let voice = Arc::new(Mutex::new(None));
        let open = Arc::new(AtomicBool::new(false));
        let (stop, stopped) = channel::<()>();
        let thread = {
            let (voice, open) = (voice.clone(), open.clone());
            std::thread::Builder::new()
                .name("lampstand-audio".into())
                .spawn(move || {
                    let _stream = open_stream(device.as_deref(), voice, open.clone());
                    // Holds the stream open until the player drops its sender.
                    let _ = stopped.recv();
                    open.store(false, Ordering::SeqCst);
                })
                .ok()
        };
        Player {
            voice,
            open,
            stop: Some(stop),
            thread,
        }
    }

    /// Whether the output is open and working.
    pub fn output_open(&self) -> bool {
        self.open.load(Ordering::SeqCst)
    }

    /// Starts `pcm` at `from` seconds in, replacing what plays.
    pub fn play(&self, pcm: Arc<Pcm>, from: f64) {
        *lock(&self.voice) = Some(Voice::new(pcm, from));
    }

    pub fn pause(&self) {
        if let Some(v) = lock(&self.voice).as_mut() {
            v.pause();
        }
    }

    pub fn resume(&self) {
        if let Some(v) = lock(&self.voice).as_mut() {
            v.resume();
        }
    }

    /// Fades out over `secs` and stops.
    pub fn stop(&self, secs: f64) {
        if let Some(v) = lock(&self.voice).as_mut() {
            v.stop(secs);
        }
    }

    /// Silence at once.
    pub fn panic(&self) {
        if let Some(v) = lock(&self.voice).as_mut() {
            v.panic();
        }
    }

    /// What is playing, or `None` when nothing is (a finished voice is
    /// cleared here).
    pub fn playing(&self) -> Option<Playing> {
        let mut slot = lock(&self.voice);
        if slot.as_ref().is_some_and(Voice::finished) {
            *slot = None;
        }
        slot.as_ref().map(|v| Playing {
            paused: v.paused(),
            ending: v.ending(),
            position: v.position(),
            length: v.length(),
        })
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // The sound ends with the player; the thread is not waited for.
        if let Some(v) = lock(&self.voice).as_mut() {
            v.panic();
        }
        drop(self.stop.take());
        drop(self.thread.take());
    }
}

fn open_stream(
    name: Option<&str>,
    voice: Arc<Mutex<Option<Voice>>>,
    open: Arc<AtomicBool>,
) -> Option<cpal::Stream> {
    let host = cpal::default_host();
    let chosen = name.and_then(|n| {
        host.output_devices()
            .ok()?
            .find(|d| d.name().is_ok_and(|x| x == n))
    });
    if name.is_some() && chosen.is_none() {
        eprintln!("lampstand-media: no sound output called {name:?}; using the default");
    }
    let Some(device) = chosen.or_else(|| host.default_output_device()) else {
        eprintln!("lampstand-media: this computer has no sound output");
        return None;
    };
    let config = match device.default_output_config() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("lampstand-media: cannot read the sound output's settings: {e}");
            return None;
        }
    };
    let format = config.sample_format();
    let config: cpal::StreamConfig = config.into();
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, &config, voice, open.clone()),
        SampleFormat::I16 => build::<i16>(&device, &config, voice, open.clone()),
        SampleFormat::U16 => build::<u16>(&device, &config, voice, open.clone()),
        other => {
            eprintln!("lampstand-media: unsupported sample format {other:?}");
            return None;
        }
    }?;
    if let Err(e) = stream.play() {
        eprintln!("lampstand-media: cannot start the sound output: {e}");
        return None;
    }
    open.store(true, Ordering::SeqCst);
    Some(stream)
}

fn build<T: SizedSample + FromSample<f32> + Send + 'static>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    voice: Arc<Mutex<Option<Voice>>>,
    open: Arc<AtomicBool>,
) -> Option<cpal::Stream> {
    let channels = usize::from(config.channels.max(1));
    let rate = config.sample_rate.0;
    let mut scratch: Vec<f32> = Vec::new();
    device
        .build_output_stream(
            config,
            move |out: &mut [T], _| {
                scratch.clear();
                scratch.resize(out.len(), 0.0);
                // A callback that cannot get the voice this instant plays
                // silence rather than wait.
                if let Ok(mut slot) = voice.try_lock() {
                    if let Some(v) = slot.as_mut() {
                        v.render(&mut scratch, channels, rate);
                    }
                }
                for (o, s) in out.iter_mut().zip(&scratch) {
                    *o = T::from_sample(*s);
                }
            },
            move |e| {
                eprintln!("lampstand-media: audio output: {e}");
                open.store(false, Ordering::SeqCst);
            },
            None,
        )
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 1000;

    /// One second of constant 0.5, mono, at 1 kHz.
    fn tone(seconds: usize) -> Arc<Pcm> {
        Arc::new(Pcm {
            rate: RATE,
            channels: 1,
            samples: vec![0.5; RATE as usize * seconds],
        })
    }

    fn render(v: &mut Voice, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames];
        v.render(&mut out, 1, RATE);
        out
    }

    #[test]
    fn plays_from_where_it_was_asked_and_finishes_at_the_end() {
        let mut v = Voice::new(tone(1), 0.9);
        let out = render(&mut v, 200);
        assert!(out[..99].iter().all(|s| *s == 0.5));
        assert!(out[110..].iter().all(|s| *s == 0.0));
        assert!(v.finished());
    }

    #[test]
    fn pause_holds_the_place_and_resume_carries_on() {
        let mut v = Voice::new(tone(2), 0.0);
        render(&mut v, 100);
        v.pause();
        render(&mut v, 100);
        assert!(v.paused());
        let held = v.position();
        assert!(render(&mut v, 100).iter().all(|s| *s == 0.0));
        assert_eq!(v.position(), held, "a paused voice does not move");
        v.resume();
        let after = render(&mut v, 100);
        assert!(!v.paused());
        assert_eq!(*after.last().unwrap(), 0.5);
        assert!(v.position() > held);
    }

    #[test]
    fn stop_fades_down_and_then_finishes() {
        let mut v = Voice::new(tone(3), 0.0);
        render(&mut v, 10);
        v.stop(0.5);
        assert!(v.ending());
        let out = render(&mut v, 400);
        assert!(out[0] > out[200] && out[200] > out[399], "gets quieter");
        assert!(!v.finished(), "still fading at 0.4 s of 0.5 s");
        render(&mut v, 200);
        assert!(v.finished());
        assert!(render(&mut v, 10).iter().all(|s| *s == 0.0));
    }

    #[test]
    fn panic_is_silent_within_a_few_milliseconds() {
        let mut v = Voice::new(tone(3), 0.0);
        render(&mut v, 10);
        v.panic();
        let out = render(&mut v, 20);
        assert!(out[10..].iter().all(|s| *s == 0.0), "{out:?}");
        assert!(v.finished());
    }

    #[test]
    fn panic_works_on_a_paused_and_on_a_fading_voice() {
        let mut paused = Voice::new(tone(3), 0.0);
        paused.pause();
        render(&mut paused, 100);
        paused.panic();
        render(&mut paused, 20);
        assert!(paused.finished());
        let mut fading = Voice::new(tone(3), 0.0);
        fading.stop(5.0);
        render(&mut fading, 50);
        fading.panic();
        render(&mut fading, 20);
        assert!(fading.finished());
    }

    #[test]
    fn a_finished_voice_cannot_be_resumed() {
        let mut v = Voice::new(tone(1), 0.0);
        v.panic();
        render(&mut v, 20);
        v.resume();
        assert!(v.finished());
        assert!(render(&mut v, 10).iter().all(|s| *s == 0.0));
    }

    #[test]
    fn stereo_keeps_channels_apart_and_folds_to_mono() {
        let pcm = Pcm {
            rate: 10,
            channels: 2,
            samples: vec![0.0, 1.0, 1.0, 0.0, 0.0, 1.0],
        };
        assert_eq!(sample(&pcm, 0.0, 1, 2), 1.0);
        assert_eq!(sample(&pcm, 0.5, 0, 2), 0.5);
        assert_eq!(sample(&pcm, 0.0, 0, 1), 0.5);
    }

    #[test]
    fn a_player_with_no_output_still_answers() {
        // On a computer with no sound device the output does not open, and
        // nothing here blocks or panics.
        let p = Player::start(Some("no such output".into()));
        p.play(tone(1), 0.0);
        p.pause();
        p.resume();
        p.stop(0.1);
        p.panic();
        let _ = p.playing();
        let _ = p.output_open();
    }

    #[test]
    fn a_file_that_is_not_sound_is_refused_with_a_reason() {
        assert!(load(std::path::Path::new("/no/such/file.wav")).is_err());
    }
}
