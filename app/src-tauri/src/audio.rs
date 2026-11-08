//! The sound deck: what plays on this device when it holds Audio (decision
//! 0010).
//!
//! The deck takes [`SoundCommand`]s, from the sync node when another device
//! asks or straight from the app when this one does, and drives one
//! [`Out`] (the player on a sound output). It answers three questions for
//! the rest of the app: what is playing, whether the output works, and why a
//! sound did not play.
//!
//! - **Files come from this device's own copy.** A cue names a file by hash;
//!   if the bytes are not here the sound does not play and the deck says why.
//! - **Decoding never blocks.** A file is decoded on a thread of its own. A
//!   stop or a panic that arrives first wins: the sound that was being
//!   decoded is then not started (each command bumps a generation, and a
//!   decode only starts its sound if the generation is still the one it was
//!   begun in).
//! - **The output opens when it is first needed,** so a device that never
//!   plays opens no sound output. Choosing another output stops what plays.
//! - **A dead output is dropped, not kept.** An output that says it died
//!   (unplugged, switched off), that could not be opened, or that has
//!   stopped asking for sound while one should be playing (the watchdog) is
//!   thrown away together with the sound on it: nothing is "playing" any
//!   more and [`AudioDeck::problem`] says the output is not working. The
//!   next Play opens a new one, and so does a gentle timer
//!   ([`AudioDeck::tick`]), so a plugged-back-in interface works again
//!   without restarting the app. Pause, fade out and the panic stop clear
//!   the screen even when the output is dead.
//! - **A new sound replaces the old one at once,** except while the old one
//!   is fading out: then the new one waits for the fade to end, for up to
//!   [`HANDOVER`]. That is how the service moving on to sound that plays on
//!   its own lets what played before fade out (#181): the app sends a short
//!   fade, then the Play.
//! - **Passing Audio stops the sound.** The app calls [`AudioDeck::cut`] when
//!   this device stops holding Audio.
//!
//! Why a sound did not play is a [`SoundProblem`] (a reason from a fixed
//! list, never a path); the words are the app's, in `handler/audio.rs`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use lampstand_core::id::BlobHash;
use lampstand_media::{LoadError, LoadKind, OutputState, Pcm, Playing, Watchdog};
use lampstand_sync::net::{SoundCommand, SoundHook, SoundNow, SoundProblem, SoundReason};

/// How a stop fades out unless told otherwise, in seconds.
pub const STOP_FADE: f64 = 3.0;

/// How long a new sound waits at most for a sound that is fading out to
/// finish (and the fade the app asks for before it, in `handler/audio.rs`).
pub const HANDOVER: Duration = Duration::from_secs(1);

/// How long after each failure in a row the deck tries the output again:
/// 5, 10, 30, then every 60 seconds. A Play, a new output or a success starts
/// the count over.
pub const REOPEN: [Duration; 4] = [
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(30),
    Duration::from_secs(60),
];

fn backoff(tries: u32) -> Duration {
    REOPEN[(tries as usize).min(REOPEN.len() - 1)]
}
/// How long a new output may take to ask for its first sound.
pub const START_GRACE: Duration = Duration::from_secs(5);
/// How long an output that was asking for sound may stop before it is
/// counted dead. Output buffers are tens of milliseconds.
pub const STALL: Duration = Duration::from_secs(2);

/// A sound output, as the deck uses it.
pub trait Out: Send {
    fn play(&mut self, pcm: Arc<Pcm>, from: f64);
    fn pause(&mut self);
    fn resume(&mut self);
    fn stop(&mut self, secs: f64);
    fn panic(&mut self);
    fn playing(&mut self) -> Option<Playing>;
    /// How the output is doing: still opening, open, or down.
    fn state(&self) -> OutputState {
        OutputState::Open
    }
    /// How many times the device has asked for sound so far, for the
    /// watchdog; `None` when the output cannot say (it is then never counted
    /// stalled, only dead when it says so).
    fn beats(&self) -> Option<u64> {
        None
    }
}

impl Out for lampstand_media::Player {
    fn play(&mut self, pcm: Arc<Pcm>, from: f64) {
        lampstand_media::Player::play(self, pcm, from);
    }
    fn pause(&mut self) {
        lampstand_media::Player::pause(self);
    }
    fn resume(&mut self) {
        lampstand_media::Player::resume(self);
    }
    fn stop(&mut self, secs: f64) {
        lampstand_media::Player::stop(self, secs);
    }
    fn panic(&mut self) {
        lampstand_media::Player::panic(self);
    }
    fn playing(&mut self) -> Option<Playing> {
        lampstand_media::Player::playing(self)
    }
    fn state(&self) -> OutputState {
        self.output_state()
    }
    fn beats(&self) -> Option<u64> {
        Some(lampstand_media::Player::beats(self))
    }
}

/// Opens the named output (the default if `None`).
pub type OpenOut = Box<dyn Fn(Option<String>) -> Box<dyn Out> + Send + Sync>;
/// The file for a hash, if its bytes are on this device.
pub type FindFile = Box<dyn Fn(&BlobHash) -> Option<PathBuf> + Send + Sync>;
pub type Decode = Arc<dyn Fn(&Path) -> Result<Arc<Pcm>, LoadError> + Send + Sync>;
pub type Job = Box<dyn FnOnce() + Send>;
/// Runs a decode: on a thread in the app, inline in tests.
pub type Spawn = Arc<dyn Fn(Job) + Send + Sync>;

/// Where the deck gets what it does not own, so tests can stand in.
pub struct Parts {
    pub open: OpenOut,
    pub path: FindFile,
    pub decode: Decode,
    pub spawn: Spawn,
}

impl Parts {
    /// The real thing: cpal output, files in the library's blob store,
    /// decoding on a thread.
    pub fn real(library: Arc<lampstand_library::Library>) -> Parts {
        Parts {
            open: Box::new(|name| Box::new(lampstand_media::Player::start(name))),
            path: Box::new(move |hash| library.repo().blobs().path(hash)),
            decode: Arc::new(lampstand_media::load_sound),
            spawn: Arc::new(|job| {
                let _ = std::thread::Builder::new()
                    .name("lampstand-decode".into())
                    .spawn(job);
            }),
        }
    }
}

struct Meta {
    item: String,
    name: String,
}

/// A sound read and ready, waiting for the one before it to fade out.
struct Queued {
    pcm: Arc<Pcm>,
    meta: Meta,
    generation: u64,
    /// When it starts even if the fade has not finished.
    by: Instant,
}

/// The output is down: why, what it was doing, and when to try it again.
struct Down {
    reason: SoundReason,
    item: String,
    name: String,
    next_try: Instant,
    /// Failures in a row, for the back-off.
    tries: u32,
}

struct Inner {
    parts: Parts,
    /// The output chosen on this device; `None` is the computer's default.
    output: Option<String>,
    out: Option<Box<dyn Out>>,
    /// Bumped by every command that ends or replaces a sound.
    generation: u64,
    /// What was asked to play, from the cue until it ends.
    current: Option<Meta>,
    /// Set by a Play that came while a sound faded out: until when the new
    /// sound waits for the fade.
    fading_until: Option<Instant>,
    /// The new sound, read, while it waits for the fade.
    queued: Option<Queued>,
    /// The sound most recently asked for, until it ends or is stopped: what
    /// a failure is about when it fails before it started.
    asked: Option<Meta>,
    /// Why the last sound did not play (the file), until the next one plays.
    problem: Option<SoundProblem>,
    /// Set while the output is down, until a new one is seen working.
    down: Option<Down>,
    /// `out` was opened after a failure and has not yet shown it works.
    probing: bool,
    watchdog: Watchdog,
    /// What was last handed to `changed`, without the position.
    told: Option<Key>,
    /// What was last handed to `problem_changed`.
    told_problem: Option<Option<SoundProblem>>,
}

impl Inner {
    /// Throws a dead output away with the sound on it, and notes why.
    fn fail(&mut self, reason: SoundReason, now: Instant) {
        let (item, name) = match self.current.as_ref().or(self.asked.as_ref()) {
            Some(m) => (m.item.clone(), m.name.clone()),
            None => (String::new(), String::new()),
        };
        let tries = self.down.as_ref().map_or(0, |d| d.tries);
        // Said once when it starts, not at every attempt.
        if self.down.as_ref().is_none_or(|d| d.reason != reason) {
            eprintln!(
                "lampstand: the sound output is not working ({reason:?}); it will be tried again"
            );
        }
        self.out = None;
        self.current = None;
        self.probing = false;
        self.watchdog.reset();
        self.down = Some(Down {
            reason,
            item,
            name,
            next_try: now + backoff(tries),
            tries: tries + 1,
        });
    }

    /// Looks at the output: drops it if it is dead, and notes when a new one
    /// has shown it works.
    fn reap(&mut self, now: Instant) {
        let Some(out) = self.out.as_mut() else {
            return;
        };
        let state = out.state();
        let wanted = self.probing || out.playing().is_some();
        let stalled = !state.down()
            && out
                .beats()
                .is_some_and(|b| self.watchdog.stalled(now, b, wanted));
        if state.down() || stalled {
            let reason = match state {
                OutputState::NoDevice => SoundReason::NoOutput,
                OutputState::Missing => SoundReason::OutputMissing,
                _ => SoundReason::OutputDown,
            };
            self.fail(reason, now);
        } else if self.probing && state == OutputState::Open {
            self.probing = false;
            self.down = None;
            self.asked = None;
        }
    }

    /// Starts a sound that is read and ready, cutting what is left of any
    /// fade.
    fn start(&mut self, pcm: Arc<Pcm>, meta: Meta) {
        self.fading_until = None;
        // The next Play is a chance to find the output working again,
        // and starts the back-off over.
        if let Some(d) = self.down.as_mut() {
            d.tries = 0;
        }
        self.reap(Instant::now());
        self.ensure_out();
        if let Some(out) = self.out.as_mut() {
            out.play(pcm, 0.0);
        }
        self.current = Some(meta);
    }

    /// Whether a sound is fading out now and a new one should wait for it.
    fn fading(&mut self, now: Instant) -> bool {
        self.fading_until.is_some_and(|until| now < until)
            && self
                .out
                .as_mut()
                .and_then(|out| out.playing())
                .is_some_and(|p| p.ending)
    }

    /// Opens the output if there is none. A new output after a failure is on
    /// trial until it shows it works.
    fn ensure_out(&mut self) {
        if self.out.is_none() {
            let chosen = self.output.clone();
            self.out = Some((self.parts.open)(chosen));
            self.watchdog.reset();
            self.probing = self.down.is_some();
        }
    }
}

/// What a screen notices about what is playing: not where in it.
type Key = Option<(String, String, bool, bool)>;

/// The deck. Cheap to clone: every clone is the same deck.
#[derive(Clone)]
pub struct AudioDeck(Arc<Mutex<Inner>>);

fn lock(m: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl AudioDeck {
    pub fn new(parts: Parts, output: Option<String>) -> AudioDeck {
        AudioDeck(Arc::new(Mutex::new(Inner {
            parts,
            output,
            out: None,
            generation: 0,
            current: None,
            fading_until: None,
            queued: None,
            asked: None,
            problem: None,
            down: None,
            probing: false,
            watchdog: Watchdog::new(START_GRACE, STALL),
            told: None,
            told_problem: None,
        })))
    }

    /// What the sync node calls when another device sends a cue.
    pub fn hook(&self) -> SoundHook {
        let deck = self.clone();
        Arc::new(move |cmd| deck.command(cmd))
    }

    /// Acts on a cue.
    pub fn command(&self, cmd: SoundCommand) {
        match cmd {
            SoundCommand::Play { file, name, item } => self.play(file, name, item),
            SoundCommand::Pause => self.with_out(|out| out.pause()),
            SoundCommand::Resume => self.with_out(|out| out.resume()),
            SoundCommand::Stop { fade_ms } => {
                lock(&self.0).generation += 1;
                self.with_out(|out| out.stop(f64::from(fade_ms) / 1000.0));
            }
            SoundCommand::Panic => self.cut(),
        }
    }

    /// Passes a command to the output, then lets a dead output go, so the
    /// screens never go on showing a sound on an output that cannot play it.
    fn with_out(&self, f: impl FnOnce(&mut dyn Out)) {
        let mut s = lock(&self.0);
        if let Some(out) = s.out.as_mut() {
            f(out.as_mut());
        }
        s.reap(Instant::now());
    }

    /// Silence now, and forget a sound that was still being decoded.
    pub fn cut(&self) {
        {
            let mut s = lock(&self.0);
            s.generation += 1;
            s.asked = None;
        }
        self.with_out(|out| out.panic());
    }

    /// Chooses the output this deck plays through. What plays stops, and an
    /// output that was down is forgotten: the new one is tried at the next
    /// Play.
    pub fn set_output(&self, name: Option<String>) {
        let mut s = lock(&self.0);
        if s.output == name {
            return;
        }
        s.generation += 1;
        s.out = None;
        s.current = None;
        s.queued = None;
        s.asked = None;
        s.down = None;
        s.probing = false;
        s.watchdog.reset();
        s.output = name;
    }

    /// Called with the clock (about ten times a second). Notices an output
    /// that died or stalled, drops it, and, every few seconds while it is
    /// down, tries a new one, so an interface that is plugged back in works
    /// again. `now` is given so the rules are tested without waiting.
    pub fn tick(&self, now: Instant) {
        let mut s = lock(&self.0);
        s.reap(now);
        // A new sound waiting for a fade starts when the fade is over.
        if let Some(q) = s.queued.take() {
            if q.generation != s.generation {
                // Stopped or cut while it waited.
            } else if s.fading(now) && now < q.by {
                s.queued = Some(q);
            } else {
                s.start(q.pcm, q.meta);
            }
        }
        let due = s.down.as_ref().is_some_and(|d| now >= d.next_try);
        if s.out.is_none() && due {
            s.ensure_out();
        }
    }

    fn play(&self, file: [u8; 32], name: String, item: String) {
        let (path, decode, spawn, generation) = {
            let mut s = lock(&self.0);
            s.generation += 1;
            s.queued = None;
            let now = Instant::now();
            let ending = s
                .out
                .as_mut()
                .and_then(|out| out.playing())
                .is_some_and(|p| p.ending && !p.paused);
            if ending {
                // A sound that is fading out finishes its fade first.
                s.fading_until = Some(now + HANDOVER);
            } else {
                // What plays gives way at once to what replaces it.
                s.fading_until = None;
                if let Some(out) = s.out.as_mut() {
                    out.panic();
                }
            }
            s.reap(now);
            s.current = None;
            s.asked = Some(Meta {
                item: item.clone(),
                name: name.clone(),
            });
            let hash = BlobHash::from_bytes(file);
            let Some(path) = (s.parts.path)(&hash) else {
                s.problem = Some(SoundProblem {
                    reason: SoundReason::NotOnDevice,
                    item,
                    name,
                });
                return;
            };
            s.problem = None;
            (
                path,
                s.parts.decode.clone(),
                s.parts.spawn.clone(),
                s.generation,
            )
        };
        let deck = self.clone();
        spawn(Box::new(move || {
            let loaded = decode(&path);
            let mut s = lock(&deck.0);
            if s.generation != generation {
                // Stopped, cut or replaced while it was being read.
                return;
            }
            let pcm = match loaded {
                Ok(pcm) => pcm,
                Err(e) => {
                    eprintln!("lampstand: no sound from {}: {e}", path.display());
                    let reason = match e.kind {
                        LoadKind::Unsupported => SoundReason::Unsupported,
                        LoadKind::Unreadable => SoundReason::Unreadable,
                    };
                    s.problem = Some(SoundProblem { reason, item, name });
                    return;
                }
            };
            let meta = Meta { item, name };
            let now = Instant::now();
            if s.fading(now) {
                let by = s.fading_until.unwrap_or(now);
                s.queued = Some(Queued {
                    pcm,
                    meta,
                    generation,
                    by,
                });
                return;
            }
            s.start(pcm, meta);
        }));
    }

    /// What is playing now (and so what to tell the other devices), or
    /// `None` for silence. A sound that has ended is forgotten here, and so
    /// is one on an output that is down.
    pub fn now(&self) -> Option<SoundNow> {
        let mut s = lock(&self.0);
        if s.out.as_ref().is_some_and(|out| out.state().down()) {
            // Not playing; `current` stays until the output is dropped, so
            // the problem can name the sound it stopped.
            return None;
        }
        let playing = s.out.as_mut().and_then(|out| out.playing());
        let Some(p) = playing else {
            // Nothing plays; a sound still being decoded is `current`'s
            // absence, not its presence. One that had played has ended.
            if s.current.take().is_some() {
                s.asked = None;
            }
            return None;
        };
        let meta = s.current.as_ref()?;
        Some(SoundNow {
            item: meta.item.clone(),
            name: meta.name.clone(),
            paused: p.paused,
            ending: p.ending,
            position_ms: (p.position * 1000.0) as u32,
            length_ms: (p.length * 1000.0) as u32,
        })
    }

    /// Like [`AudioDeck::now`], but only when the state changed since the
    /// last time this was called (a new sound, a pause, a fade, the end),
    /// not as the position moves. `Some(state)` is what to tell everyone.
    pub fn changed(&self) -> Option<Option<SoundNow>> {
        let now = self.now();
        let key: Key = now
            .as_ref()
            .map(|n| (n.item.clone(), n.name.clone(), n.paused, n.ending));
        let mut s = lock(&self.0);
        if s.told.as_ref() == Some(&key) {
            return None;
        }
        s.told = Some(key);
        Some(now)
    }

    /// Whether the output works; true until it is first needed. False from
    /// the moment it is known dead until a new one is seen working.
    pub fn output_ok(&self) -> bool {
        let s = lock(&self.0);
        s.down.is_none() && s.out.as_ref().is_none_or(|out| !out.state().down())
    }

    /// Why the last sound did not play, or that the output is not working,
    /// until it does. An output that is down comes first: nothing can play
    /// until it is back.
    pub fn problem(&self) -> Option<SoundProblem> {
        let s = lock(&self.0);
        match &s.down {
            Some(d) => Some(SoundProblem {
                reason: d.reason,
                item: d.item.clone(),
                name: d.name.clone(),
            }),
            None => s.problem.clone(),
        }
    }

    /// Like [`AudioDeck::problem`], but only when it changed since the last
    /// time this was called. `Some(problem)` is what to tell everyone
    /// (`Some(None)`: it is over).
    pub fn problem_changed(&self) -> Option<Option<SoundProblem>> {
        let problem = self.problem();
        let mut s = lock(&self.0);
        if s.told_problem.as_ref() == Some(&problem) {
            return None;
        }
        s.told_problem = Some(problem.clone());
        Some(problem)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// An output that records what it is told, and can die, stall and come
    /// back.
    #[derive(Default)]
    struct Log {
        calls: Vec<String>,
        playing: Option<Playing>,
        /// `None` is open and working.
        state: Option<OutputState>,
        /// The count the output reports, when it counts.
        beats: Option<u64>,
        /// A dead output renders nothing, so a fade or a panic never
        /// finishes (the real player clears a known-dead one at once; this
        /// fake does not, to show that the deck does not rely on it).
        dead: bool,
    }

    struct Fake(Arc<Mutex<Log>>);

    impl Out for Fake {
        fn play(&mut self, _: Arc<Pcm>, from: f64) {
            let mut l = self.0.lock().unwrap();
            l.calls.push(format!("play@{from}"));
            l.playing = Some(Playing {
                paused: false,
                ending: false,
                position: 1.5,
                length: 60.0,
            });
        }
        fn pause(&mut self) {
            let mut l = self.0.lock().unwrap();
            l.calls.push("pause".into());
            if l.dead {
                return;
            }
            if let Some(p) = l.playing.as_mut() {
                p.paused = true;
            }
        }
        fn resume(&mut self) {
            let mut l = self.0.lock().unwrap();
            l.calls.push("resume".into());
            if let Some(p) = l.playing.as_mut() {
                p.paused = false;
            }
        }
        fn stop(&mut self, secs: f64) {
            let mut l = self.0.lock().unwrap();
            l.calls.push(format!("stop {secs}"));
            if l.dead {
                return;
            }
            if let Some(p) = l.playing.as_mut() {
                p.ending = true;
            }
        }
        fn panic(&mut self) {
            let mut l = self.0.lock().unwrap();
            l.calls.push("panic".into());
            if !l.dead {
                l.playing = None;
            }
        }
        fn playing(&mut self) -> Option<Playing> {
            self.0.lock().unwrap().playing
        }
        fn state(&self) -> OutputState {
            self.0.lock().unwrap().state.unwrap_or(OutputState::Open)
        }
        fn beats(&self) -> Option<u64> {
            self.0.lock().unwrap().beats
        }
    }

    struct Rig {
        deck: AudioDeck,
        log: Arc<Mutex<Log>>,
        opened: Arc<Mutex<Vec<Option<String>>>>,
        /// Decodes waiting to run (when `hold` is set).
        held: Arc<Mutex<Vec<Job>>>,
    }

    const HERE: [u8; 32] = [1; 32];

    fn unreadable() -> LoadError {
        LoadError {
            kind: LoadKind::Unreadable,
            detail: "not sound".into(),
        }
    }

    fn rig(hold: bool, output: Option<String>) -> Rig {
        let log = Arc::new(Mutex::new(Log::default()));
        let opened = Arc::new(Mutex::new(Vec::new()));
        let held: Arc<Mutex<Vec<Job>>> = Arc::new(Mutex::new(Vec::new()));
        let files: HashMap<BlobHash, PathBuf> =
            HashMap::from([(BlobHash::from_bytes(HERE), PathBuf::from("/files/here.mp3"))]);
        let (l, o, h) = (log.clone(), opened.clone(), held.clone());
        let parts = Parts {
            open: Box::new(move |name| {
                o.lock().unwrap().push(name);
                Box::new(Fake(l.clone()))
            }),
            path: Box::new(move |hash| files.get(hash).cloned()),
            decode: Arc::new(|path| {
                if path.ends_with("broken.mp3") {
                    Err(unreadable())
                } else {
                    Ok(Arc::new(Pcm {
                        rate: 10,
                        channels: 1,
                        samples: vec![0.0; 10],
                    }))
                }
            }),
            spawn: Arc::new(move |job| {
                if hold {
                    h.lock().unwrap().push(job);
                } else {
                    job();
                }
            }),
        };
        Rig {
            deck: AudioDeck::new(parts, output),
            log,
            opened,
            held,
        }
    }

    fn play(name: &str) -> SoundCommand {
        SoundCommand::Play {
            file: HERE,
            name: name.into(),
            item: "item-1".into(),
        }
    }

    fn calls(r: &Rig) -> Vec<String> {
        r.log.lock().unwrap().calls.clone()
    }

    #[test]
    fn a_play_cue_starts_the_file_and_the_state_names_it() {
        let r = rig(false, None);
        assert!(r.deck.now().is_none());
        assert!(
            r.opened.lock().unwrap().is_empty(),
            "no output until needed"
        );
        r.deck.command(play("Walk-in music"));
        assert_eq!(calls(&r), ["play@0"]);
        let now = r.deck.now().unwrap();
        assert_eq!(now.name, "Walk-in music");
        assert_eq!(now.item, "item-1");
        assert_eq!((now.position_ms, now.length_ms), (1500, 60_000));
        assert!(r.deck.problem().is_none());
        assert_eq!(r.opened.lock().unwrap().len(), 1);
    }

    #[test]
    fn the_chosen_output_is_the_one_opened_and_choosing_another_stops_the_sound() {
        let r = rig(false, Some("Mixer USB".into()));
        r.deck.command(play("A"));
        assert_eq!(*r.opened.lock().unwrap(), [Some("Mixer USB".to_string())]);
        r.deck.set_output(Some("Mixer USB".into()));
        assert!(r.deck.now().is_some(), "the same output changes nothing");
        r.deck.set_output(None);
        assert!(
            r.deck.now().is_none(),
            "a new output starts without the old sound"
        );
        r.deck.command(play("B"));
        assert_eq!(r.opened.lock().unwrap().last(), Some(&None));
    }

    #[test]
    fn pause_resume_and_stop_reach_the_output() {
        let r = rig(false, None);
        r.deck.command(play("A"));
        r.deck.command(SoundCommand::Pause);
        assert!(r.deck.now().unwrap().paused);
        r.deck.command(SoundCommand::Resume);
        assert!(!r.deck.now().unwrap().paused);
        r.deck.command(SoundCommand::Stop { fade_ms: 2500 });
        assert!(r.deck.now().unwrap().ending);
        assert!(calls(&r).contains(&"stop 2.5".to_string()));
        r.deck.command(SoundCommand::Panic);
        assert!(r.deck.now().is_none());
    }

    #[test]
    fn a_file_that_is_not_here_says_so_and_plays_nothing() {
        let r = rig(false, None);
        r.deck.command(SoundCommand::Play {
            file: [9; 32],
            name: "Walk-in music".into(),
            item: "i".into(),
        });
        assert!(r.deck.now().is_none());
        assert_eq!(
            r.deck.problem(),
            Some(SoundProblem {
                reason: SoundReason::NotOnDevice,
                item: "i".into(),
                name: "Walk-in music".into(),
            })
        );
        assert!(r.opened.lock().unwrap().is_empty());
        // The next sound that plays clears it.
        r.deck.command(play("Next"));
        assert!(r.deck.problem().is_none());
    }

    #[test]
    fn a_file_that_will_not_decode_says_so() {
        let mut r = rig(false, None);
        let files = HashMap::from([(
            BlobHash::from_bytes(HERE),
            PathBuf::from("/files/broken.mp3"),
        )]);
        let log = r.log.clone();
        let parts = Parts {
            open: Box::new(move |_| Box::new(Fake(log.clone()))),
            path: Box::new(move |h| files.get(h).cloned()),
            decode: Arc::new(|_| Err(unreadable())),
            spawn: Arc::new(|job| job()),
        };
        r.deck = AudioDeck::new(parts, None);
        r.deck.command(play("Track"));
        assert!(r.deck.now().is_none());
        assert_eq!(
            r.deck.problem(),
            Some(SoundProblem {
                reason: SoundReason::Unreadable,
                item: "item-1".into(),
                name: "Track".into(),
            })
        );
    }

    #[test]
    fn a_kind_of_file_that_cannot_be_played_is_said_apart_from_a_damaged_one() {
        let r = rig(false, None);
        let files = HashMap::from([(BlobHash::from_bytes(HERE), PathBuf::from("/files/a.wma"))]);
        let log = r.log.clone();
        let deck = AudioDeck::new(
            Parts {
                open: Box::new(move |_| Box::new(Fake(log.clone()))),
                path: Box::new(move |h| files.get(h).cloned()),
                decode: Arc::new(|_| {
                    Err(LoadError {
                        kind: LoadKind::Unsupported,
                        detail: "Windows Media Audio (WMA) isn't supported".into(),
                    })
                }),
                spawn: Arc::new(|job| job()),
            },
            None,
        );
        deck.command(play("Hymn"));
        assert_eq!(deck.problem().unwrap().reason, SoundReason::Unsupported);
        assert!(deck.output_ok(), "a bad file is not a bad output");
    }

    #[test]
    fn a_stop_or_a_panic_that_comes_first_beats_a_decode_still_running() {
        for cmd in [SoundCommand::Panic, SoundCommand::Stop { fade_ms: 100 }] {
            let r = rig(true, None);
            r.deck.command(play("Slow file"));
            assert!(r.deck.now().is_none(), "still being read");
            r.deck.command(cmd);
            for job in r.held.lock().unwrap().drain(..) {
                job();
            }
            assert!(
                r.deck.now().is_none() && calls(&r).iter().all(|c| !c.starts_with("play")),
                "the late sound is not started"
            );
        }
    }

    #[test]
    fn a_newer_play_replaces_one_still_being_read() {
        let r = rig(true, None);
        r.deck.command(play("First"));
        r.deck.command(play("Second"));
        let jobs: Vec<_> = r.held.lock().unwrap().drain(..).collect();
        for job in jobs {
            job();
        }
        assert_eq!(r.deck.now().unwrap().name, "Second");
        assert_eq!(
            calls(&r).iter().filter(|c| c.starts_with("play")).count(),
            1
        );
    }

    #[test]
    fn a_new_sound_cuts_the_old_one_at_once() {
        let r = rig(false, None);
        r.deck.command(play("A"));
        r.deck.command(play("B"));
        assert_eq!(calls(&r), ["play@0", "panic", "play@0"]);
        assert_eq!(r.deck.now().unwrap().name, "B");
    }

    #[test]
    fn a_new_sound_waits_for_one_that_is_fading_out() {
        let r = rig(false, None);
        r.deck.command(play("A"));
        r.deck.command(SoundCommand::Stop { fade_ms: 1000 });
        r.deck.command(play("B"));
        assert_eq!(calls(&r), ["play@0", "stop 1"], "A fades; B waits");
        r.deck.tick(Instant::now());
        assert_eq!(calls(&r).len(), 2, "still fading");
        // The fade ends: B starts at the next tick.
        r.log.lock().unwrap().playing = None;
        r.deck.tick(Instant::now());
        assert_eq!(calls(&r), ["play@0", "stop 1", "play@0"]);
        assert_eq!(r.deck.now().unwrap().name, "B");
    }

    #[test]
    fn a_fade_that_does_not_finish_in_time_is_cut_for_the_new_sound() {
        let r = rig(false, None);
        r.deck.command(play("A"));
        r.deck.command(SoundCommand::Stop { fade_ms: 3000 });
        r.deck.command(play("B"));
        r.deck.tick(Instant::now() + HANDOVER);
        assert_eq!(calls(&r), ["play@0", "stop 3", "play@0"]);
        assert_eq!(r.deck.now().unwrap().name, "B");
    }

    #[test]
    fn a_stop_or_a_panic_while_a_sound_waits_for_a_fade_drops_it() {
        for cmd in [SoundCommand::Stop { fade_ms: 3000 }, SoundCommand::Panic] {
            let r = rig(false, None);
            r.deck.command(play("A"));
            r.deck.command(SoundCommand::Stop { fade_ms: 1000 });
            r.deck.command(play("B"));
            r.deck.command(cmd);
            r.log.lock().unwrap().playing = None;
            r.deck.tick(Instant::now() + HANDOVER);
            assert_eq!(
                calls(&r).iter().filter(|c| c.starts_with("play")).count(),
                1,
                "B never starts"
            );
            assert!(r.deck.now().is_none());
        }
    }

    #[test]
    fn cut_silences_whatever_plays() {
        let r = rig(false, None);
        r.deck.command(play("A"));
        r.deck.cut();
        assert!(r.deck.now().is_none());
    }

    #[test]
    fn changed_speaks_only_when_the_state_changes_not_as_the_position_moves() {
        let r = rig(false, None);
        // Silence at the start is not news.
        assert_eq!(r.deck.changed(), Some(None));
        assert_eq!(r.deck.changed(), None);
        r.deck.command(play("A"));
        assert!(matches!(r.deck.changed(), Some(Some(n)) if n.name == "A"));
        r.log.lock().unwrap().playing.as_mut().unwrap().position = 9.0;
        assert_eq!(r.deck.changed(), None, "position alone is not a change");
        r.deck.command(SoundCommand::Pause);
        assert!(matches!(r.deck.changed(), Some(Some(n)) if n.paused));
        r.deck.command(SoundCommand::Panic);
        assert_eq!(r.deck.changed(), Some(None));
        assert_eq!(r.deck.changed(), None);
    }

    #[test]
    fn a_sound_that_ends_by_itself_is_forgotten() {
        let r = rig(false, None);
        r.deck.command(play("A"));
        r.log.lock().unwrap().playing = None;
        assert!(r.deck.now().is_none());
        assert_eq!(r.deck.changed().map(|n| n.is_none()), Some(true));
    }

    #[test]
    fn the_hook_is_the_same_deck() {
        let r = rig(false, None);
        (r.deck.hook())(play("A"));
        assert!(r.deck.now().is_some());
    }

    // ---- A dead output (issue #162) ---------------------------------------

    /// A rig whose every `open` hands out a new fake, so a test can tell
    /// the output that died from the one that replaced it.
    struct Outs {
        deck: AudioDeck,
        /// The fakes opened so far, oldest first.
        logs: Arc<Mutex<Vec<Arc<Mutex<Log>>>>>,
        /// What the next output opened starts as (`None`: working).
        next_state: Arc<Mutex<Option<OutputState>>>,
    }

    fn outs() -> Outs {
        let logs: Arc<Mutex<Vec<Arc<Mutex<Log>>>>> = Arc::new(Mutex::new(Vec::new()));
        let next_state = Arc::new(Mutex::new(None));
        let files: HashMap<BlobHash, PathBuf> =
            HashMap::from([(BlobHash::from_bytes(HERE), PathBuf::from("/files/here.mp3"))]);
        let (l, n) = (logs.clone(), next_state.clone());
        let deck = AudioDeck::new(
            Parts {
                open: Box::new(move |_| {
                    let log = Arc::new(Mutex::new(Log {
                        state: *n.lock().unwrap(),
                        beats: Some(0),
                        ..Log::default()
                    }));
                    l.lock().unwrap().push(log.clone());
                    Box::new(Fake(log))
                }),
                path: Box::new(move |h| files.get(h).cloned()),
                decode: Arc::new(|_| {
                    Ok(Arc::new(Pcm {
                        rate: 10,
                        channels: 1,
                        samples: vec![0.0; 10],
                    }))
                }),
                spawn: Arc::new(|job| job()),
            },
            None,
        );
        Outs {
            deck,
            logs,
            next_state,
        }
    }

    impl Outs {
        fn log(&self, n: usize) -> Arc<Mutex<Log>> {
            self.logs.lock().unwrap()[n].clone()
        }
        fn opened(&self) -> usize {
            self.logs.lock().unwrap().len()
        }
    }

    fn at(base: Instant, secs: u64) -> Instant {
        base + Duration::from_secs(secs)
    }

    #[test]
    fn an_output_that_dies_mid_play_is_dropped_and_the_sound_is_not_playing() {
        let o = outs();
        o.deck.command(play("Walk-in music"));
        o.log(0).lock().unwrap().beats = Some(50);
        assert!(o.deck.now().is_some());
        assert!(o.deck.output_ok());
        // The interface is unplugged: the stream says so.
        o.log(0).lock().unwrap().state = Some(OutputState::Failed);
        assert!(
            o.deck.now().is_none(),
            "not playing from the moment it is known dead"
        );
        assert!(!o.deck.output_ok());
        o.deck.tick(Instant::now());
        assert!(o.deck.now().is_none());
        assert!(!o.deck.output_ok());
        assert_eq!(
            o.deck.problem(),
            Some(SoundProblem {
                reason: SoundReason::OutputDown,
                item: "item-1".into(),
                name: "Walk-in music".into(),
            }),
            "the problem knows which sound it stopped"
        );
        // The screens are told, once.
        assert_eq!(o.deck.changed(), Some(None));
        assert!(o.deck.problem_changed().unwrap().is_some());
        assert_eq!(o.deck.problem_changed(), None);
    }

    #[test]
    fn the_next_play_opens_a_new_output_and_the_problem_goes_when_it_works() {
        let o = outs();
        o.deck.command(play("A"));
        o.log(0).lock().unwrap().state = Some(OutputState::Failed);
        o.deck.tick(Instant::now());
        assert_eq!(o.opened(), 1);
        // The interface is back; the volunteer presses Play again.
        o.deck.command(play("B"));
        assert_eq!(o.opened(), 2, "a new output, not the dead one");
        assert_eq!(o.deck.now().unwrap().name, "B");
        // The banner stays until the new output has shown it works (here: at
        // the next look).
        o.deck.tick(Instant::now());
        assert!(o.deck.output_ok());
        assert!(o.deck.problem().is_none());
        assert_eq!(o.deck.problem_changed(), Some(None));
        // The old output was never touched again after it died.
        assert_eq!(
            o.log(0)
                .lock()
                .unwrap()
                .calls
                .iter()
                .filter(|c| c.starts_with("play"))
                .count(),
            1
        );
    }

    #[test]
    fn a_timer_reopens_the_output_so_a_replugged_interface_recovers_without_a_play() {
        let o = outs();
        let t = Instant::now();
        o.deck.command(play("A"));
        o.log(0).lock().unwrap().state = Some(OutputState::Failed);
        o.deck.tick(t);
        assert!(!o.deck.output_ok());
        // Too soon: nothing is tried.
        o.deck.tick(at(t, 2));
        assert_eq!(o.opened(), 1);
        // Still unplugged at the first try: down again, and tried later.
        *o.next_state.lock().unwrap() = Some(OutputState::NoDevice);
        o.deck.tick(at(t, 6));
        assert_eq!(o.opened(), 2);
        o.deck.tick(at(t, 6));
        assert!(!o.deck.output_ok());
        assert_eq!(o.deck.problem().unwrap().reason, SoundReason::NoOutput);
        o.deck.tick(at(t, 12));
        assert_eq!(o.opened(), 2, "gentle: the second wait is 10 s");
        // Plugged back in: the next try works and the banner goes.
        *o.next_state.lock().unwrap() = None;
        o.deck.tick(at(t, 17));
        assert_eq!(o.opened(), 3);
        o.deck.tick(at(t, 17));
        assert!(o.deck.output_ok());
        assert!(o.deck.problem().is_none());
        assert_eq!(o.deck.problem_changed(), Some(None));
        // And nothing is tried again once it works.
        o.deck.tick(at(t, 60));
        assert_eq!(o.opened(), 3);
    }

    #[test]
    fn the_retries_back_off_to_once_a_minute_and_a_play_starts_them_over() {
        let o = outs();
        let t = Instant::now();
        *o.next_state.lock().unwrap() = Some(OutputState::Failed);
        o.deck.command(play("A"));
        o.deck.tick(t);
        // Failures at 0; tries at 5, 15, 45, 105, 165 ...
        let mut now = 0u64;
        for wait in [5u64, 10, 30, 60, 60] {
            let before = o.opened();
            o.deck.tick(at(t, now + wait - 1));
            assert_eq!(o.opened(), before, "not before {wait} s");
            now += wait;
            o.deck.tick(at(t, now));
            assert_eq!(o.opened(), before + 1, "at {wait} s");
            o.deck.tick(at(t, now));
        }
        // A Play opens at once and starts the count over.
        let before = o.opened();
        o.deck.command(play("B"));
        assert_eq!(o.opened(), before + 1);
        o.deck.tick(at(t, now));
        let after = o.opened();
        o.deck.tick(at(t, now + 5));
        assert_eq!(o.opened(), after + 1, "5 s again");
    }

    #[test]
    fn a_chosen_output_that_is_not_connected_stays_down_and_recovers_when_it_is_back() {
        let o = outs();
        let t = Instant::now();
        o.deck.set_output(Some("Mixer USB".into()));
        *o.next_state.lock().unwrap() = Some(OutputState::Missing);
        o.deck.command(play("Walk-in music"));
        o.deck.tick(t);
        assert!(o.deck.now().is_none(), "nothing plays on another output");
        assert!(!o.deck.output_ok());
        let p = o.deck.problem().unwrap();
        assert_eq!(p.reason, SoundReason::OutputMissing);
        assert_eq!(p.name, "Walk-in music", "names the sound that failed");
        // Every try is for the chosen output only, and the banner stays.
        for secs in [6u64, 20, 70, 140] {
            o.deck.tick(at(t, secs));
            assert!(!o.deck.output_ok());
            assert_eq!(o.deck.problem().unwrap().reason, SoundReason::OutputMissing);
        }
        // Plugged back in: it recovers on the timer, without a Play.
        *o.next_state.lock().unwrap() = None;
        o.deck.tick(at(t, 210));
        o.deck.tick(at(t, 210));
        assert!(o.deck.output_ok() && o.deck.problem().is_none());
        o.deck.command(play("Again"));
        assert!(o.deck.now().is_some());
    }

    #[test]
    fn the_problem_names_the_sound_that_failed_not_an_earlier_one() {
        let o = outs();
        let t = Instant::now();
        o.deck.command(play("First"));
        o.log(0).lock().unwrap().state = Some(OutputState::Failed);
        o.deck.tick(t);
        assert_eq!(o.deck.problem().unwrap().name, "First");
        *o.next_state.lock().unwrap() = Some(OutputState::Failed);
        o.deck.command(play("Second"));
        o.deck.tick(t);
        assert_eq!(o.deck.problem().unwrap().name, "Second");
    }

    #[test]
    fn an_output_that_will_not_open_is_reported_and_tried_again() {
        let o = outs();
        *o.next_state.lock().unwrap() = Some(OutputState::Failed);
        o.deck.command(play("A"));
        assert_eq!(o.opened(), 1);
        o.deck.tick(Instant::now());
        assert!(o.deck.now().is_none());
        assert!(!o.deck.output_ok());
        assert_eq!(o.deck.problem().unwrap().reason, SoundReason::OutputDown);
        // It opens next time.
        *o.next_state.lock().unwrap() = None;
        o.deck.command(play("A"));
        o.deck.tick(Instant::now());
        assert!(o.deck.output_ok() && o.deck.problem().is_none());
        assert!(o.deck.now().is_some());
    }

    #[test]
    fn a_stream_that_stops_asking_for_sound_is_dropped_by_the_watchdog() {
        let o = outs();
        let t = Instant::now();
        o.deck.command(play("A"));
        // It asks for a while...
        for n in 1..=5u64 {
            o.log(0).lock().unwrap().beats = Some(n * 100);
            o.deck.tick(at(t, n));
            assert!(o.deck.now().is_some(), "still playing at {n} s");
        }
        // ...then silently stops, with no error: the sound would seem to
        // play forever.
        o.deck.tick(at(t, 6));
        o.deck.tick(at(t, 7));
        assert!(o.deck.now().is_some(), "not yet two seconds");
        o.deck.tick(at(t, 8));
        assert!(o.deck.now().is_none());
        assert!(!o.deck.output_ok());
        assert_eq!(o.deck.problem().unwrap().reason, SoundReason::OutputDown);
    }

    #[test]
    fn a_paused_sound_on_a_healthy_stream_is_not_mistaken_for_a_dead_one() {
        let o = outs();
        let t = Instant::now();
        o.deck.command(play("A"));
        o.deck.command(SoundCommand::Pause);
        for n in 1..=60u64 {
            o.log(0).lock().unwrap().beats = Some(n * 100);
            o.deck.tick(at(t, n));
        }
        assert!(o.deck.output_ok());
        assert!(o.deck.now().unwrap().paused);
    }

    #[test]
    fn nothing_playing_is_never_a_stall() {
        let o = outs();
        let t = Instant::now();
        o.deck.command(play("A"));
        o.log(0).lock().unwrap().playing = None;
        for n in 1..=30u64 {
            o.deck.tick(at(t, n));
        }
        assert!(o.deck.output_ok() && o.deck.problem().is_none());
    }

    #[test]
    fn pause_fade_and_panic_clear_the_screen_even_when_the_stream_is_dead() {
        for cmd in [
            SoundCommand::Pause,
            SoundCommand::Resume,
            SoundCommand::Stop { fade_ms: 3000 },
            SoundCommand::Panic,
        ] {
            let o = outs();
            o.deck.command(play("A"));
            {
                let log = o.log(0);
                let mut l = log.lock().unwrap();
                l.dead = true;
                l.state = Some(OutputState::Failed);
            }
            o.deck.command(cmd.clone());
            assert!(o.deck.now().is_none(), "{cmd:?} still shows a sound");
            assert!(!o.deck.output_ok());
            assert_eq!(o.deck.problem().unwrap().reason, SoundReason::OutputDown);
        }
    }

    #[test]
    fn choosing_another_output_forgets_a_dead_one() {
        let o = outs();
        o.deck.command(play("A"));
        o.log(0).lock().unwrap().state = Some(OutputState::Failed);
        o.deck.tick(Instant::now());
        assert!(!o.deck.output_ok());
        o.deck.set_output(Some("Mixer USB".into()));
        assert!(o.deck.output_ok() && o.deck.problem().is_none());
        o.deck.command(play("B"));
        assert!(o.deck.now().is_some());
    }

    #[test]
    fn a_sound_decoding_when_the_output_died_plays_on_a_new_one() {
        let r = rig(true, None);
        r.deck.command(play("Slow file"));
        for job in r.held.lock().unwrap().drain(..) {
            job();
        }
        r.log.lock().unwrap().state = Some(OutputState::Failed);
        r.deck.command(play("Again"));
        for job in r.held.lock().unwrap().drain(..) {
            job();
        }
        // (This rig's fakes share one log, so the new output reads as failed
        // too; the point is that the deck did not wedge on the dead one.)
        assert_eq!(r.opened.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_church_that_never_plays_sound_opens_nothing_and_has_no_problem() {
        let o = outs();
        let t = Instant::now();
        for n in 0..100u64 {
            o.deck.tick(at(t, n));
        }
        assert_eq!(o.opened(), 0);
        assert!(o.deck.output_ok() && o.deck.problem().is_none());
        assert_eq!(o.deck.problem_changed(), Some(None));
        assert_eq!(o.deck.problem_changed(), None);
    }
}
