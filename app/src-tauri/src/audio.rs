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
//! - **Passing Audio stops the sound.** The app calls [`AudioDeck::cut`] when
//!   this device stops holding Audio.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use lampstand_core::id::BlobHash;
use lampstand_media::{Pcm, Playing};
use lampstand_sync::net::{SoundCommand, SoundHook, SoundNow};

/// How a stop fades out unless told otherwise, in seconds.
pub const STOP_FADE: f64 = 3.0;

/// A sound output, as the deck uses it.
pub trait Out: Send {
    fn play(&mut self, pcm: Arc<Pcm>, from: f64);
    fn pause(&mut self);
    fn resume(&mut self);
    fn stop(&mut self, secs: f64);
    fn panic(&mut self);
    fn playing(&mut self) -> Option<Playing>;
    /// Whether the output opened and works.
    fn open(&self) -> bool;
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
    fn open(&self) -> bool {
        self.output_open()
    }
}

/// Opens the named output (the default if `None`).
pub type OpenOut = Box<dyn Fn(Option<String>) -> Box<dyn Out> + Send + Sync>;
/// The file for a hash, if its bytes are on this device.
pub type FindFile = Box<dyn Fn(&BlobHash) -> Option<PathBuf> + Send + Sync>;
pub type Decode = Arc<dyn Fn(&Path) -> Result<Arc<Pcm>, String> + Send + Sync>;
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

struct Inner {
    parts: Parts,
    /// The output chosen on this device; `None` is the computer's default.
    output: Option<String>,
    out: Option<Box<dyn Out>>,
    /// Bumped by every command that ends or replaces a sound.
    generation: u64,
    /// What was asked to play, from the cue until it ends.
    current: Option<Meta>,
    problem: Option<String>,
    /// What was last handed to `changed`, without the position.
    told: Option<Key>,
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
            problem: None,
            told: None,
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
            SoundCommand::Pause => {
                if let Some(out) = lock(&self.0).out.as_mut() {
                    out.pause();
                }
            }
            SoundCommand::Resume => {
                if let Some(out) = lock(&self.0).out.as_mut() {
                    out.resume();
                }
            }
            SoundCommand::Stop { fade_ms } => {
                let mut s = lock(&self.0);
                s.generation += 1;
                if let Some(out) = s.out.as_mut() {
                    out.stop(f64::from(fade_ms) / 1000.0);
                }
            }
            SoundCommand::Panic => self.cut(),
        }
    }

    /// Silence now, and forget a sound that was still being decoded.
    pub fn cut(&self) {
        let mut s = lock(&self.0);
        s.generation += 1;
        if let Some(out) = s.out.as_mut() {
            out.panic();
        }
    }

    /// Chooses the output this deck plays through. What plays stops.
    pub fn set_output(&self, name: Option<String>) {
        let mut s = lock(&self.0);
        if s.output == name {
            return;
        }
        s.generation += 1;
        s.out = None;
        s.current = None;
        s.output = name;
    }

    fn play(&self, file: [u8; 32], name: String, item: String) {
        let (path, decode, spawn, generation) = {
            let mut s = lock(&self.0);
            s.generation += 1;
            // What plays gives way at once to what replaces it.
            if let Some(out) = s.out.as_mut() {
                out.panic();
            }
            s.current = None;
            let hash = BlobHash::from_bytes(file);
            let Some(path) = (s.parts.path)(&hash) else {
                s.problem = Some(format!("{name} is not on this device yet."));
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
                    s.problem = Some(format!("{name} could not be played."));
                    return;
                }
            };
            if s.out.is_none() {
                let chosen = s.output.clone();
                s.out = Some((s.parts.open)(chosen));
            }
            if let Some(out) = s.out.as_mut() {
                out.play(pcm, 0.0);
            }
            s.current = Some(Meta { item, name });
        }));
    }

    /// What is playing now (and so what to tell the other devices), or
    /// `None` for silence. A sound that has ended is forgotten here.
    pub fn now(&self) -> Option<SoundNow> {
        let mut s = lock(&self.0);
        let playing = s.out.as_mut().and_then(|out| out.playing());
        let Some(p) = playing else {
            // Nothing plays; a sound still being decoded is `current`'s
            // absence, not its presence.
            s.current = None;
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

    /// Whether the output opened and works; true until it is first needed.
    pub fn output_ok(&self) -> bool {
        lock(&self.0).out.as_ref().is_none_or(|out| out.open())
    }

    /// Why the last sound did not play, in words, until the next one plays.
    pub fn problem(&self) -> Option<String> {
        lock(&self.0).problem.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// An output that records what it is told.
    #[derive(Default)]
    struct Log {
        calls: Vec<String>,
        playing: Option<Playing>,
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
            if let Some(p) = l.playing.as_mut() {
                p.ending = true;
            }
        }
        fn panic(&mut self) {
            let mut l = self.0.lock().unwrap();
            l.calls.push("panic".into());
            l.playing = None;
        }
        fn playing(&mut self) -> Option<Playing> {
            self.0.lock().unwrap().playing
        }
        fn open(&self) -> bool {
            true
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
                    Err("not sound".into())
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
            r.deck.problem().as_deref(),
            Some("Walk-in music is not on this device yet.")
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
            decode: Arc::new(|_| Err("not sound".into())),
            spawn: Arc::new(|job| job()),
        };
        r.deck = AudioDeck::new(parts, None);
        r.deck.command(play("Track"));
        assert!(r.deck.now().is_none());
        assert_eq!(
            r.deck.problem().as_deref(),
            Some("Track could not be played.")
        );
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
}
