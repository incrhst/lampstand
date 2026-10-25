//! The Audio role in the app: asking for sound, and telling the screens what
//! plays (decision 0010).
//!
//! Any device can ask for a sound to play or stop; where it plays is the
//! sync node's decision (`SyncLink::route_sound`): here when this device
//! holds Audio or is alone, otherwise on the device that does. The words
//! shown when it cannot be done are in this file.
//!
//! A song's backing track that "plays on its own" starts when the song goes
//! live. A music-only item has no slides, so the live engine never takes it
//! live: it plays when someone presses play (`SoundRequest::Play`).

use lampstand_api::{Event, Response, SoundOutputs, SoundPlaying, SoundRequest, SoundView};
use lampstand_core::service::{ItemId, ItemKind, Sound, SoundStart};
use lampstand_sync::net::{SoundCommand, SoundNow, SoundRoute};

use super::{err, App, Result};
use crate::audio::{AudioDeck, STOP_FADE};

/// What a screen is told when a sound cannot go where it should.
pub const AUDIO_AWAY: &str = "The device that runs the sound is not connected.";
pub const AUDIO_NOBODY: &str = "Nobody runs the sound.";
pub const AUDIO_NOT_ALLOWED: &str = "This device cannot start or control the sound.";

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

    fn sound_of(&self, item: ItemId, backing: bool) -> Result<Sound> {
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

    /// An item went live: a song's backing track that plays on its own
    /// starts now.
    pub(super) fn cue_sound(&mut self, item: ItemId) {
        let Ok(sound) = self.sound_of(item, true) else {
            return;
        };
        if sound.start != SoundStart::OnItsOwn {
            return;
        }
        let Ok(cmd) = self.sound_cue_for(item, true) else {
            return;
        };
        if let Err(message) = self.route_sound(cmd) {
            // The screens show it (`SoundView::away` and `nobody`).
            eprintln!("lampstand: the backing track did not start: {message}");
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
        let (playing_now, problem) = if here {
            (self.audio.now().as_ref().map(playing), self.audio.problem())
        } else {
            (playing_now, None)
        };
        SoundView {
            here,
            holder,
            away,
            nobody,
            output_ok: !here || self.audio.output_ok(),
            playing: playing_now,
            problem,
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
        if let (true, Some(link)) = (here, &link) {
            if let Some(now) = self.audio.changed() {
                link.report_sound(now);
            }
        }
        let view = self.sound_view();
        if self.told_sound.as_ref() != Some(&view) {
            self.told_sound = Some(view.clone());
            (self.emit)(Event::Sound(Box::new(view)));
        }
    }
}
