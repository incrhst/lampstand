//! The Audio holder's player: one piece of sound at a time (music, a
//! backing track, a video's sound) that can be paused, stopped with a fade,
//! or cut at once.
//!
//! [`Voice`] is the sound itself and has no device in it, so its rules are
//! tested without one. [`Player`] puts a voice on an output device chosen by
//! name. The device's stream stays on a thread of its own (a cpal stream
//! must stay on the thread that made it), so nothing here waits on audio
//! hardware.

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

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

/// Why a sound file did not load, in the terms the screens care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadKind {
    /// A kind of file Lampstand cannot play.
    Unsupported,
    /// The file could not be read, or is damaged, or holds no sound.
    Unreadable,
}

/// A sound file that did not load: what kind of trouble, and the details for
/// the log (never for a screen: they can hold a path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError {
    pub kind: LoadKind,
    pub detail: String,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.detail)
    }
}

/// The file extensions of the kinds of sound the decoder is built to play:
/// MP3, WAV, FLAC, M4A (AAC or ALAC), Ogg (Vorbis or Opus), and also AIFF,
/// CAF and Matroska audio. The words shown to people name the first five.
pub const PLAYABLE_EXTENSIONS: &[&str] = &[
    "mp3", "wav", "flac", "m4a", "mp4", "aac", "ogg", "oga", "opus", "aif", "aiff", "caf", "mka",
    "mkv", "webm",
];

fn known_extension(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| PLAYABLE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Decodes the audio of the file at `path`, or says why not.
pub fn load(path: &std::path::Path) -> Result<Arc<Pcm>, LoadError> {
    let unreadable = |detail: String| LoadError {
        kind: LoadKind::Unreadable,
        detail,
    };
    let bytes = Arc::new(std::fs::read(path).map_err(|e| unreadable(e.to_string()))?);
    match deckcraft_media::audio::decode(&bytes) {
        Ok(pcm) if pcm.frames() > 0 => Ok(Arc::new(pcm)),
        Ok(_) => Err(unreadable("the file has no sound".into())),
        // A file whose kind we do play, that will not decode, is damaged; only
        // a kind we do not play is "unsupported".
        Err(deckcraft_media::MediaError::Unsupported(what)) if !known_extension(path) => {
            Err(LoadError {
                kind: LoadKind::Unsupported,
                detail: format!("{what} isn't supported"),
            })
        }
        Err(e) => Err(unreadable(e.to_string())),
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

/// How an output device is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputState {
    /// Still being opened.
    Opening,
    /// Open and (as far as the device has said) working.
    Open,
    /// This computer has no sound output at all.
    NoDevice,
    /// A particular output was chosen and is not connected. Nothing is played
    /// on any other output meanwhile.
    Missing,
    /// The output could not be opened, or died (unplugged, switched off, its
    /// driver gave up).
    Failed,
}

impl OutputState {
    /// Whether the output is known to be down.
    pub fn down(self) -> bool {
        matches!(
            self,
            OutputState::NoDevice | OutputState::Failed | OutputState::Missing
        )
    }

    fn code(self) -> u8 {
        match self {
            OutputState::Opening => 0,
            OutputState::Open => 1,
            OutputState::NoDevice => 2,
            OutputState::Failed => 3,
            OutputState::Missing => 4,
        }
    }

    fn from_code(code: u8) -> OutputState {
        match code {
            1 => OutputState::Open,
            2 => OutputState::NoDevice,
            3 => OutputState::Failed,
            4 => OutputState::Missing,
            _ => OutputState::Opening,
        }
    }
}

/// Where the audio thread and the device's callbacks tell the player how
/// the output is doing.
#[derive(Clone)]
struct Health {
    state: Arc<AtomicU8>,
    /// Counts the device's render callbacks: a stream that stops calling
    /// back has stopped, whether or not it said so.
    beats: Arc<AtomicU64>,
}

impl Health {
    fn new() -> Health {
        Health {
            state: Arc::new(AtomicU8::new(OutputState::Opening.code())),
            beats: Arc::new(AtomicU64::new(0)),
        }
    }

    fn set(&self, state: OutputState) {
        self.state.store(state.code(), Ordering::SeqCst);
    }

    /// A dead stream stays dead: a late "open" does not undo a failure.
    fn opened(&self) {
        let _ = self.state.compare_exchange(
            OutputState::Opening.code(),
            OutputState::Open.code(),
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }
}

/// Notices an output that has stopped calling back while a sound is meant to
/// be playing. The caller gives the time, so the rule is tested without
/// waiting.
///
/// A stream that has never called back gets the longer `grace` (devices can
/// be slow to start); one that was calling back and stopped gets `stall`.
#[derive(Debug, Clone)]
pub struct Watchdog {
    grace: Duration,
    stall: Duration,
    last: u64,
    /// When the count last moved, or when a sound was first meant to play.
    since: Option<Instant>,
}

impl Watchdog {
    pub fn new(grace: Duration, stall: Duration) -> Watchdog {
        Watchdog {
            grace,
            stall,
            last: 0,
            since: None,
        }
    }

    /// Looks at the callback count `beats` at time `now`. `wanted` says a
    /// sound is meant to be playing; when nothing is, there is nothing to
    /// stall and the clock starts over. True when the output has stalled.
    pub fn stalled(&mut self, now: Instant, beats: u64, wanted: bool) -> bool {
        if !wanted {
            self.since = None;
            self.last = beats;
            return false;
        }
        let since = *self.since.get_or_insert(now);
        if beats != self.last {
            self.last = beats;
            self.since = Some(now);
            return false;
        }
        let limit = if beats == 0 { self.grace } else { self.stall };
        now.saturating_duration_since(since) > limit
    }

    /// Forgets everything (a new output).
    pub fn reset(&mut self) {
        self.last = 0;
        self.since = None;
    }
}

/// A voice on an output device. Dropping the player stops the sound.
pub struct Player {
    voice: Arc<Mutex<Option<Voice>>>,
    health: Health,
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Player {
    /// Opens the output called `device` (the default output if `None`, or if
    /// no output has that name). Never waits: [`Player::output_state`] says
    /// whether the output came up.
    pub fn start(device: Option<String>) -> Player {
        let voice = Arc::new(Mutex::new(None));
        let health = Health::new();
        let (stop, stopped) = channel::<()>();
        let thread = {
            let (voice, health) = (voice.clone(), health.clone());
            std::thread::Builder::new()
                .name("lampstand-audio".into())
                .spawn(move || {
                    let _stream = open_stream(device.as_deref(), voice, &health);
                    // Holds the stream open until the player drops its sender.
                    let _ = stopped.recv();
                })
                .ok()
        };
        if thread.is_none() {
            health.set(OutputState::Failed);
        }
        Player {
            voice,
            health,
            stop: Some(stop),
            thread,
        }
    }

    /// How the output is doing.
    pub fn output_state(&self) -> OutputState {
        OutputState::from_code(self.health.state.load(Ordering::SeqCst))
    }

    /// Whether the output is open and working.
    pub fn output_open(&self) -> bool {
        self.output_state() == OutputState::Open
    }

    /// How many times the device has asked for sound. A count that stops
    /// moving while a sound plays is an output that has stopped.
    pub fn beats(&self) -> u64 {
        self.health.beats.load(Ordering::Relaxed)
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
        if self.output_state().down() {
            // Nothing renders, so a fade would never finish.
            *lock(&self.voice) = None;
        } else if let Some(v) = lock(&self.voice).as_mut() {
            v.stop(secs);
        }
    }

    /// Silence at once.
    pub fn panic(&self) {
        if self.output_state().down() {
            *lock(&self.voice) = None;
        } else if let Some(v) = lock(&self.voice).as_mut() {
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
    health: &Health,
) -> Option<cpal::Stream> {
    let failed = |state: OutputState| {
        health.set(state);
        None
    };
    let host = cpal::default_host();
    let chosen = name.and_then(|n| {
        host.output_devices()
            .ok()?
            .find(|d| d.name().is_ok_and(|x| x == n))
    });
    if name.is_some() && chosen.is_none() {
        // A chosen output that is not there stays silent: music must not
        // come out of the speakers or the projector instead.
        eprintln!("lampstand-media: the chosen sound output {name:?} is not connected");
        return failed(OutputState::Missing);
    }
    let Some(device) = chosen.or_else(|| host.default_output_device()) else {
        eprintln!("lampstand-media: this computer has no sound output");
        return failed(OutputState::NoDevice);
    };
    let config = match device.default_output_config() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("lampstand-media: cannot read the sound output's settings: {e}");
            return failed(OutputState::Failed);
        }
    };
    let format = config.sample_format();
    let config: cpal::StreamConfig = config.into();
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, &config, voice, health.clone()),
        SampleFormat::I16 => build::<i16>(&device, &config, voice, health.clone()),
        SampleFormat::U16 => build::<u16>(&device, &config, voice, health.clone()),
        other => {
            eprintln!("lampstand-media: unsupported sample format {other:?}");
            return failed(OutputState::Failed);
        }
    };
    let Some(stream) = stream else {
        return failed(OutputState::Failed);
    };
    if let Err(e) = stream.play() {
        eprintln!("lampstand-media: cannot start the sound output: {e}");
        return failed(OutputState::Failed);
    }
    health.opened();
    Some(stream)
}

fn build<T: SizedSample + FromSample<f32> + Send + 'static>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    voice: Arc<Mutex<Option<Voice>>>,
    health: Health,
) -> Option<cpal::Stream> {
    let channels = usize::from(config.channels.max(1));
    let rate = config.sample_rate.0;
    let mut scratch: Vec<f32> = Vec::new();
    let beats = health.beats.clone();
    device
        .build_output_stream(
            config,
            move |out: &mut [T], _| {
                beats.fetch_add(1, Ordering::Relaxed);
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
                // A device that is unplugged or switched off says so here;
                // whoever owns the player then drops it.
                eprintln!("lampstand-media: audio output: {e}");
                health.set(OutputState::Failed);
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
        let _ = p.output_state();
        let _ = p.beats();
    }

    #[test]
    fn a_file_that_is_not_sound_is_refused_with_a_reason() {
        let e = load(std::path::Path::new("/no/such/file.wav")).unwrap_err();
        assert_eq!(e.kind, LoadKind::Unreadable);
    }

    #[test]
    fn a_file_that_is_not_audio_at_all_is_a_kind_of_file_that_cannot_be_played() {
        let dir = std::env::temp_dir().join(format!("lampstand-load-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notes.mp3");
        std::fs::write(&path, b"this is not music").unwrap();
        let e = load(&path).unwrap_err();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            e.kind,
            LoadKind::Unreadable,
            "an .mp3 that will not decode is damaged"
        );
    }

    #[test]
    fn a_kind_of_file_that_is_not_played_is_unsupported() {
        let dir = std::env::temp_dir().join(format!("lampstand-wma-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hymn.wma");
        // The header of an ASF (Windows Media) file.
        std::fs::write(
            &path,
            [
                0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62,
                0xCE, 0x6C, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
        )
        .unwrap();
        let e = load(&path).unwrap_err();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(e.kind, LoadKind::Unsupported);
    }

    /// A tiny WAV: the decoder takes it, and the list of kinds names it.
    #[test]
    fn a_wav_file_plays_and_the_list_of_kinds_names_what_the_decoder_takes() {
        let mut wav = Vec::new();
        let samples: Vec<i16> = (0..800).map(|n| (n % 50) as i16 * 100).collect();
        let data = samples.len() * 2;
        wav.extend(b"RIFF");
        wav.extend((36 + data as u32).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(8000u32.to_le_bytes());
        wav.extend(16000u32.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend((data as u32).to_le_bytes());
        for s in samples {
            wav.extend(s.to_le_bytes());
        }
        let dir = std::env::temp_dir().join(format!("lampstand-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        std::fs::write(&path, &wav).unwrap();
        let pcm = load(&path).expect("a WAV plays");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(pcm.frames() > 0);
        for kind in ["mp3", "wav", "flac", "m4a", "ogg"] {
            assert!(PLAYABLE_EXTENSIONS.contains(&kind), "{kind}");
        }
    }

    fn secs(base: Instant, n: u64) -> Instant {
        base + Duration::from_secs(n)
    }

    fn dog() -> Watchdog {
        Watchdog::new(Duration::from_secs(5), Duration::from_secs(2))
    }

    #[test]
    fn the_watchdog_is_quiet_while_the_output_keeps_calling_back() {
        let t = Instant::now();
        let mut w = dog();
        for n in 0..30u64 {
            assert!(!w.stalled(secs(t, n), n + 1, true));
        }
    }

    #[test]
    fn the_watchdog_trips_when_the_count_stops_for_longer_than_the_stall() {
        let t = Instant::now();
        let mut w = dog();
        assert!(!w.stalled(secs(t, 0), 10, true));
        assert!(!w.stalled(secs(t, 1), 20, true));
        // Stuck at 20 from here.
        assert!(!w.stalled(secs(t, 2), 20, true));
        assert!(
            !w.stalled(secs(t, 3), 20, true),
            "exactly the stall is not over it"
        );
        assert!(w.stalled(secs(t, 4), 20, true));
    }

    #[test]
    fn a_stream_that_never_called_back_gets_the_longer_grace() {
        let t = Instant::now();
        let mut w = dog();
        assert!(!w.stalled(secs(t, 0), 0, true));
        assert!(!w.stalled(secs(t, 4), 0, true));
        assert!(!w.stalled(secs(t, 5), 0, true));
        assert!(w.stalled(secs(t, 6), 0, true));
    }

    #[test]
    fn the_watchdog_does_not_count_time_when_nothing_is_meant_to_play() {
        let t = Instant::now();
        let mut w = dog();
        assert!(!w.stalled(secs(t, 0), 5, false));
        assert!(!w.stalled(secs(t, 100), 5, false));
        // A sound starts now: the clock starts now, not 100 s ago.
        assert!(!w.stalled(secs(t, 101), 5, true));
        assert!(!w.stalled(secs(t, 102), 5, true));
        assert!(w.stalled(secs(t, 104), 5, true));
    }

    #[test]
    fn a_stream_that_comes_back_is_forgiven_and_reset_starts_over() {
        let t = Instant::now();
        let mut w = dog();
        assert!(!w.stalled(secs(t, 0), 3, true));
        assert!(w.stalled(secs(t, 10), 3, true));
        assert!(!w.stalled(secs(t, 11), 4, true), "it called back");
        w.reset();
        assert!(!w.stalled(secs(t, 30), 0, true));
        assert!(!w.stalled(secs(t, 34), 0, true));
    }

    #[test]
    fn output_states_round_trip_and_say_when_they_are_down() {
        for s in [
            OutputState::Opening,
            OutputState::Open,
            OutputState::NoDevice,
            OutputState::Failed,
            OutputState::Missing,
        ] {
            assert_eq!(OutputState::from_code(s.code()), s);
        }
        assert!(OutputState::Failed.down() && OutputState::NoDevice.down());
        assert!(!OutputState::Opening.down() && !OutputState::Open.down());
    }

    #[test]
    fn a_late_open_does_not_undo_a_failure() {
        let h = Health::new();
        h.set(OutputState::Failed);
        h.opened();
        assert_eq!(
            OutputState::from_code(h.state.load(Ordering::SeqCst)),
            OutputState::Failed
        );
    }

    #[test]
    fn stop_and_panic_clear_the_voice_at_once_when_the_output_is_known_dead() {
        // A player with no thread behind it, whose output has failed.
        let health = Health::new();
        health.set(OutputState::Failed);
        let (stop, _stopped) = channel::<()>();
        let p = Player {
            voice: Arc::new(Mutex::new(None)),
            health,
            stop: Some(stop),
            thread: None,
        };
        for end in [0, 1] {
            p.play(tone(5), 0.0);
            assert!(p.playing().is_some(), "a dead stream never finishes it");
            if end == 0 {
                p.stop(3.0);
            } else {
                p.panic();
            }
            assert!(p.playing().is_none());
        }
    }
}
