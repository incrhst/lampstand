//! The Audio role in the app: asking for sound, and telling the screens what
//! plays (decision 0010).
//!
//! Any device can ask for a sound to play or stop; where it plays is the
//! sync node's decision (`SyncLink::route_sound`): here when this device
//! holds Audio or is alone, otherwise on the device that does. The words
//! shown when it cannot be done are in this file.
//!
//! Sound that "plays on its own" starts when its item goes live: a song's
//! backing track with the song, a music-only item when `Next` reaches it
//! (#181). Sound that waits plays when someone presses play
//! (`SoundRequest::Play`). Starting sound on its own lets what was playing
//! fade out for a second first ([`HANDOVER`]): the fade, then the Play, go
//! by the same route as a press of a button, so they reach the device that
//! runs the sound wherever it is.

use lampstand_api::{Event, Response, SoundOutputs, SoundPlaying, SoundRequest, SoundView};
use lampstand_core::service::{ItemId, ItemKind, Sound, SoundStart};
use lampstand_sync::net::{SoundCommand, SoundNow, SoundProblem, SoundReason, SoundRoute};

use super::{err, App, Result};
use crate::audio::{AudioDeck, HANDOVER, STOP_FADE};

/// What a screen is told when a sound cannot go where it should.
pub const AUDIO_AWAY: &str = "The device that runs the sound is not connected.";
pub const AUDIO_NOBODY: &str = "Nobody runs the sound.";
pub const AUDIO_NOT_ALLOWED: &str = "This device cannot start or control the sound.";

/// The kinds of file the player can open (what the decoder is built with),
/// for the words below.
pub const FORMATS: &str = "MP3, WAV, FLAC, M4A and Ogg";

/// The words for a sound that did not play, in one place. `device` is the
/// name of the device that runs the sound when it is not this one, so the
/// words name it; `None` is this computer.
///
/// A reason about the sound (the file) says which sound first; one about
/// the output does so only when a sound was playing. No path or error text
/// from the other device ever gets here: only a reason from a fixed list
/// and the sound's name.
pub fn problem_words(problem: &SoundProblem, device: Option<&str>) -> String {
    let subject = match device {
        Some(name) => name.to_string(),
        None => "This computer".to_string(),
    };
    let what = problem.name.trim();
    let reason = match problem.reason {
        SoundReason::NotOnDevice => match device {
            Some(name) => format!(
                "The sound file isn't on {name} yet. Wait for it to arrive, or add the file again."
            ),
            None => "The sound file isn't on this computer yet. Add the file again, or wait for it to come from another computer.".to_string(),
        },
        SoundReason::Unsupported => {
            format!("{subject} can't play this kind of file. Lampstand can play {FORMATS}.")
        }
        SoundReason::Unreadable => format!(
            "{subject} couldn't read the sound file. It may be damaged. Try adding it to the service again."
        ),
        SoundReason::OutputDown => match device {
            Some(name) => format!(
                "The sound output on {name} isn't working. Check it is plugged in and switched on."
            ),
            None => "The sound output on this computer isn't working. Check it is plugged in and switched on, or choose another in Setup › Sound.".to_string(),
        },
        SoundReason::OutputMissing => match device {
            Some(name) => format!(
                "The sound output chosen on {name} isn't connected. Plug it in, or choose another in Setup › Sound."
            ),
            None => "The sound output you chose isn't connected. Plug it in, or choose another in Setup › Sound.".to_string(),
        },
        SoundReason::NoOutput => {
            format!("{subject} has no sound output. Plug one in and switch it on.")
        }
    };
    match (what.is_empty(), problem.reason) {
        (true, _) => reason,
        (false, SoundReason::NotOnDevice | SoundReason::Unsupported | SoundReason::Unreadable) => {
            format!("{what} couldn't be played. {reason}")
        }
        (false, SoundReason::OutputDown | SoundReason::NoOutput | SoundReason::OutputMissing) => {
            format!("{what} isn't playing. {reason}")
        }
    }
}

fn playing(now: &SoundNow) -> SoundPlaying {
    SoundPlaying {
        item: now.item.clone(),
        name: now.name.clone(),
        paused: now.paused,
        ending: now.ending,
        position_ms: now.position_ms,
        length_ms: now.length_ms,
    }
}

impl App {
    /// The deck that plays here, for the shell to hand to the sync node.
    pub fn audio(&self) -> &AudioDeck {
        &self.audio
    }

    /// For tests: stands another deck in for the one that plays.
    #[doc(hidden)]
    pub fn set_audio(&mut self, deck: AudioDeck) {
        self.audio = deck;
    }

    pub(super) fn sound_request(&mut self, request: SoundRequest) -> Result<Response> {
        match request {
            SoundRequest::Play { item, backing } => {
                let cmd = self.sound_cue_for(item, backing)?;
                self.route_sound(cmd)?;
            }
            SoundRequest::Pause => self.route_sound(SoundCommand::Pause)?,
            SoundRequest::Resume => self.route_sound(SoundCommand::Resume)?,
            SoundRequest::Stop => self.route_sound(SoundCommand::Stop {
                fade_ms: (STOP_FADE * 1000.0) as u32,
            })?,
            SoundRequest::Panic => self.route_sound(SoundCommand::Panic)?,
            SoundRequest::Outputs => {
                return Ok(Response::SoundOutputs(SoundOutputs {
                    devices: lampstand_media::output_devices(),
                    default: lampstand_media::default_output(),
                    chosen: self.settings.get().sound_output.clone(),
                }));
            }
            SoundRequest::Status => {}
        }
        // Anything that was asked may have changed what the screens show.
        self.audio_tick();
        Ok(Response::Sound(self.sound_view()))
    }

    /// The cue that plays `item` (a music-only item, or the song's backing
    /// track) from the loaded service.
    fn sound_cue_for(&self, item: ItemId, backing: bool) -> Result<SoundCommand> {
        let sound = self.sound_of(item, backing)?;
        Ok(SoundCommand::Play {
            file: *sound.file.as_bytes(),
            name: sound.name,
            item: item.to_string(),
        })
    }

    pub(super) fn sound_of(&self, item: ItemId, backing: bool) -> Result<Sound> {
        let service = self
            .loaded
            .as_ref()
            .map(|l| l.view.service)
            .ok_or("no service is open")?;
        let service = self
            .library
            .service(service)
            .map_err(err)?
            .ok_or("no such service")?;
        let found = service
            .ordered()
            .into_iter()
            .find(|(id, _)| *id == item)
            .map(|(_, i)| i)
            .ok_or("no such item")?;
        match (backing, &found.kind, &found.backing) {
            (true, _, Some(sound)) => Ok(sound.clone()),
            (false, ItemKind::Sound { sound }, _) => Ok(sound.clone()),
            _ => Err("that item has no sound".into()),
        }
    }

    /// Plays here or sends it on, and says in words when it cannot be done.
    fn route_sound(&mut self, cmd: SoundCommand) -> Result<()> {
        let route = match self.sync_link() {
            Some(link) => link.route_sound(cmd.clone()),
            None => SoundRoute::Local,
        };
        match route {
            SoundRoute::Local => {
                self.audio.command(cmd);
                Ok(())
            }
            SoundRoute::Sent => Ok(()),
            SoundRoute::Away => Err(AUDIO_AWAY.into()),
            SoundRoute::Nobody => Err(AUDIO_NOBODY.into()),
            SoundRoute::NotAllowed => Err(AUDIO_NOT_ALLOWED.into()),
        }
    }

    /// An item went live: its sound starts now if it plays on its own (a
    /// music-only item, or a song's backing track). What plays fades out
    /// first.
    pub(super) fn cue_sound(&mut self, item: ItemId) {
        let found = self
            .sound_of(item, false)
            .map(|sound| (false, sound))
            .or_else(|_| self.sound_of(item, true).map(|sound| (true, sound)));
        let Ok((backing, sound)) = found else {
            return;
        };
        // It is already playing (Play was pressed before the service got
        // here), or paused: it is left as it is, not started again.
        let current = self.sound_view().playing;
        if current
            .as_ref()
            .is_some_and(|p| p.item == item.to_string() && !p.ending)
        {
            return;
        }
        if sound.start != SoundStart::OnItsOwn {
            return;
        }
        let Ok(cmd) = self.sound_cue_for(item, backing) else {
            return;
        };
        let fade = current.is_some_and(|p| !p.paused && !p.ending);
        let started = if fade {
            self.route_sound(SoundCommand::Stop {
                fade_ms: HANDOVER.as_millis() as u32,
            })
            .and_then(|()| self.route_sound(cmd))
        } else {
            self.route_sound(cmd)
        };
        if let Err(message) = started {
            // The screens show it (`SoundView::away` and `nobody`).
            eprintln!("lampstand: {} did not start: {message}", sound.name);
        }
    }

    /// What the screens show of the sound.
    pub fn sound_view(&self) -> SoundView {
        let link = self.sync_link();
        let remote = link.as_ref().and_then(|l| l.audio_remote());
        let here = link.as_ref().is_none_or(|l| l.audio_here());
        let (holder, away, playing_now) = match &remote {
            Some(r) => (
                r.name.clone(),
                !r.connected,
                r.playing.as_ref().map(playing),
            ),
            None => (String::new(), false, None),
        };
        let nobody = !here && remote.is_none();
        // Why a sound did not play: said where it plays, and sent from there
        // to the devices that show the sound (the Operator, Projection). A
        // holder that is away says nothing new: it is said to be away.
        let (playing_now, problem) = if here {
            let problem = self.audio.problem().or_else(|| {
                // The output is known dead but not yet dropped.
                (!self.audio.output_ok()).then(|| SoundProblem {
                    reason: SoundReason::OutputDown,
                    item: String::new(),
                    name: String::new(),
                })
            });
            (
                self.audio.now().as_ref().map(playing),
                problem.map(|p| (problem_words(&p, None), p.item)),
            )
        } else {
            let problem = remote.as_ref().filter(|r| r.connected).and_then(|r| {
                r.problem
                    .as_ref()
                    .map(|p| (problem_words(p, Some(&r.name)), p.item.clone()))
            });
            (playing_now, problem)
        };
        let (problem, problem_item) = match problem {
            Some((words, item)) => (Some(words), Some(item).filter(|i| !i.is_empty())),
            None => (None, None),
        };
        SoundView {
            here,
            holder,
            away,
            nobody,
            output_ok: !here || self.audio.output_ok(),
            playing: playing_now,
            problem,
            problem_item,
        }
    }

    /// Called with the clock: tells the other devices what plays here when
    /// that changes, stops the sound when this device no longer holds Audio,
    /// and tells the screen when what it shows changed.
    pub(super) fn audio_tick(&mut self) {
        let link = self.sync_link();
        let here = link.as_ref().is_none_or(|l| l.audio_here());
        if !here && self.audio.now().is_some() {
            // Passing Audio stops what is playing.
            self.audio.cut();
        }
        if here && !self.audio_held {
            // Audio has just come here: say again what is wrong.
            if let Some(link) = &link {
                link.resend_sound_problem();
            }
        }
        self.audio_held = here;
        if here {
            // A dead output is dropped, and tried again every few seconds.
            self.audio.tick(std::time::Instant::now());
        }
        if let (true, Some(link)) = (here, &link) {
            if let Some(now) = self.audio.changed() {
                link.report_sound(now);
            }
            if let Some(problem) = self.audio.problem_changed() {
                link.report_sound_problem(problem);
            }
        }
        let view = self.sound_view();
        if self.told_sound.as_ref() != Some(&view) {
            self.told_sound = Some(view.clone());
            (self.emit)(Event::Sound(Box::new(view)));
        }
    }
}
