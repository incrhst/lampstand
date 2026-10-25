//! The church's shared settings in the app: the look (brand, logo bug),
//! the brand and default themes and the default idle screen follow the
//! church to its paired devices through the synced church-settings
//! document (`lampstand_core::church`). Everything else in
//! `DeviceSettings` and the outputs stays on this machine.
//!
//! Until this device has met another one it keeps these settings in
//! `settings.json`, as it always did, and `DeviceSettings::church_synced`
//! is false. The first time a paired device is connected, `church_tick`
//! settles which copy is the church's:
//!
//! | this device | the document | what happens |
//! | --- | --- | --- |
//! | nothing | anything | the document is used |
//! | settings | nothing | this device's settings go into the document |
//! | settings | the same | the document is used |
//! | settings | different | the person is asked once (`ChurchStatus::conflict`) |
//!
//! From then on the document is the only copy and `settings.json` holds
//! none. Edits made after that merge by field: different fields from
//! different devices both stay, the same field keeps the later edit, and a
//! deleted theme the settings name falls back to the Starter theme.
//!
//! Pictures and videos the settings point at are fetched from the other
//! devices (blobs never travel inside documents); until they arrive the
//! screens show the plain theme background and Setup says what is awaited.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use lampstand_api::{ChurchChoice, ChurchConflict, ChurchStatus, DeviceSettings, WaitingFile};
use lampstand_core::brand::Brand;
use lampstand_core::church::ChurchSettings;
use lampstand_core::id::BlobHash;
use lampstand_core::output::IdleScreen;
use lampstand_core::theme::{Background, ThemeId};

use super::{err, App, Result};

/// What the app needs of the sync node, so the handler stays free of the
/// network and tests can stand in for it.
pub trait SyncLink: Send + Sync {
    /// This device's name as other devices see it: "Mary's laptop".
    fn device_name(&self) -> String;
    /// This device's key, in hex.
    fn device_key(&self) -> String;
    /// The keys, in hex, of the paired devices connected now.
    fn connected(&self) -> Vec<String>;
    /// Starts fetching these files from whichever connected device has
    /// them, without waiting.
    fn fetch(&self, hashes: Vec<BlobHash>);
    /// Whether this device has screens set up (an output turned on): only
    /// then can it hold the Projection role.
    fn screens(&self, _set_up: bool) {}
    /// Where a sound cue made here goes: played here (the default, for a
    /// device with nobody to share with), or sent to the Audio holder.
    fn route_sound(
        &self,
        _cmd: lampstand_sync::net::SoundCommand,
    ) -> lampstand_sync::net::SoundRoute {
        lampstand_sync::net::SoundRoute::Local
    }
    /// Tells the other devices what this one, as the Audio holder, is
    /// playing (`None`: nothing).
    fn report_sound(&self, _now: Option<lampstand_sync::net::SoundNow>) {}
    /// Tells the other devices why a sound did not play here, as the Audio
    /// holder (`None`: nothing is wrong now).
    fn report_sound_problem(&self, _problem: Option<lampstand_sync::net::SoundProblem>) {}
    /// Audio has just come to this device: says again what is wrong.
    fn resend_sound_problem(&self) {}
    /// Whether this device plays the sound: it holds Audio, or has nobody to
    /// share with.
    fn audio_here(&self) -> bool {
        true
    }
    /// When another device holds Audio: its name, whether it is connected,
    /// and what it last said it is playing.
    fn audio_remote(&self) -> Option<AudioRemote> {
        None
    }
}

/// The device that plays the sound, when it is not this one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioRemote {
    pub name: String,
    pub connected: bool,
    pub playing: Option<lampstand_sync::net::SoundNow>,
    /// Why the last sound did not play there, as it said (only while it is
    /// connected).
    pub problem: Option<lampstand_sync::net::SoundProblem>,
}

/// A first-time agreement waits this long for the other device's copy to
/// arrive, the device with the lower key first, so that two devices that
/// both have settings do not both offer them at once.
const SETTLE: Duration = Duration::from_secs(3);
const SETTLE_LATER: Duration = Duration::from_secs(6);
/// How often the settled state and the awaited files are looked at.
const LOOK: Duration = Duration::from_millis(500);
/// How long before a file that did not arrive is asked for again.
const ASK_AGAIN: Duration = Duration::from_secs(10);

/// The shared settings as the rest of the app reads them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Church {
    pub brand: Option<Brand>,
    pub brand_theme: Option<ThemeId>,
    pub default_theme: Option<ThemeId>,
    pub idle: Option<IdleScreen>,
}

impl Church {
    fn from_doc(doc: &ChurchSettings) -> Self {
        Church {
            brand: doc.brand(),
            brand_theme: doc.brand_theme,
            default_theme: doc.default_theme,
            idle: doc.idle(),
        }
    }

    fn from_local(settings: &DeviceSettings) -> Self {
        Church {
            brand: settings.brand.clone(),
            brand_theme: settings.brand_theme,
            default_theme: settings.default_theme,
            idle: settings.idle.clone(),
        }
    }

    fn put_in_doc(&self, doc: &mut ChurchSettings) {
        doc.set_brand(self.brand.as_ref());
        doc.brand_theme = self.brand_theme;
        doc.default_theme = self.default_theme;
        doc.set_idle(self.idle.as_ref());
    }

    fn is_empty(&self) -> bool {
        *self == Church::default()
    }

    /// One line on what is set: "Grace Chapel, logo, Looping video: Loop.mp4".
    fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(b) = &self.brand {
            parts.push(if b.name.trim().is_empty() {
                "church look".to_string()
            } else {
                b.name.trim().to_string()
            });
            if b.logo.is_some() {
                parts.push("logo".into());
            }
        }
        match &self.idle {
            Some(IdleScreen::Image { name, .. }) => parts.push(format!("Picture: {name}")),
            Some(IdleScreen::Video { name, .. }) => parts.push(format!("Looping video: {name}")),
            Some(IdleScreen::DeckSlide { slide, .. }) => {
                parts.push(format!("Slide {} as the idle screen", slide + 1))
            }
            _ => {}
        }
        if parts.is_empty() && self.default_theme.is_some() {
            parts.push("a default theme".into());
        }
        parts.join(", ")
    }
}

/// What to do with this device's own settings (`mine`) when it first meets
/// a church document (`shared`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agree {
    /// Use the document; this device's settings are dropped or the same.
    UseShared,
    /// Put this device's settings in the document.
    OfferMine,
    /// Both have different settings: ask.
    Ask,
}

pub fn agree(mine: &Church, shared: &Church) -> Agree {
    if mine == shared || mine.is_empty() {
        Agree::UseShared
    } else if shared.is_empty() {
        Agree::OfferMine
    } else {
        Agree::Ask
    }
}

/// The app's bookkeeping about the shared settings.
#[derive(Default)]
pub struct ChurchSync {
    link: Option<std::sync::Arc<dyn SyncLink>>,
    /// When the first-time agreement may be made without a document from
    /// the other device.
    settle_at: Option<Instant>,
    looked: Option<Instant>,
    /// Files asked for, and when.
    asked: HashSet<BlobHash>,
    asked_at: Option<Instant>,
    /// Files the settings point at that are not on this device.
    waiting: Vec<(BlobHash, &'static str)>,
    /// Another device's differing settings, waiting for a choice.
    conflict: Option<ChurchConflict>,
}

impl App {
    /// Called by the shell once sync is running.
    pub fn set_sync_link(&mut self, link: std::sync::Arc<dyn SyncLink>) {
        self.church_sync.link = Some(link);
        self.tell_screens();
    }

    /// The link to the sync node, once sync is running.
    pub(super) fn sync_link(&self) -> Option<std::sync::Arc<dyn SyncLink>> {
        self.church_sync.link.clone()
    }

    /// Tells sync whether this device has screens set up.
    pub(super) fn tell_screens(&self) {
        if let Some(link) = &self.church_sync.link {
            link.screens(self.outputs.all().iter().any(|o| o.enabled));
        }
    }

    /// The shared settings as they stand: from the document once this
    /// device has agreed with the church, else this device's own.
    pub(super) fn church(&self) -> Church {
        let settings = self.settings.get();
        if settings.church_synced {
            Church::from_doc(&self.library.church_settings().unwrap_or_default())
        } else {
            Church::from_local(settings)
        }
    }

    /// `DeviceSettings` for the UI: this device's own settings and the
    /// church's, with a default or brand theme that has been deleted
    /// shown as unset.
    pub(super) fn device_settings(&self) -> DeviceSettings {
        let church = self.church();
        let live =
            |id: Option<ThemeId>| id.filter(|id| matches!(self.library.theme(*id), Ok(Some(_))));
        DeviceSettings {
            brand: church.brand,
            brand_theme: live(church.brand_theme),
            default_theme: live(church.default_theme),
            idle: church.idle,
            ..self.settings.get().clone()
        }
    }

    /// Changes the shared settings: in the document once agreed, else in
    /// this device's own file.
    pub(super) fn save_church(&mut self, change: impl FnOnce(&mut Church)) -> Result<()> {
        let mut church = self.church();
        change(&mut church);
        self.write_church(church)
    }

    fn write_church(&mut self, church: Church) -> Result<()> {
        if self.settings.get().church_synced {
            let before = self.library.church_settings().map_err(err)?;
            let mut doc = before.clone();
            church.put_in_doc(&mut doc);
            if doc != before {
                doc.updated_by = self.church_sync.link.as_ref().map(|l| l.device_name());
                self.library.put_church_settings(&doc).map_err(err)?;
            }
            Ok(())
        } else {
            let mut settings = self.settings.get().clone();
            settings.brand = church.brand;
            settings.brand_theme = church.brand_theme;
            settings.default_theme = church.default_theme;
            settings.idle = church.idle;
            self.settings.put(settings).map_err(err)
        }
    }

    /// Takes a deleted theme out of the settings that name it.
    pub(super) fn church_forget_theme(&mut self, id: ThemeId) -> Result<()> {
        if self.settings.get().church_synced {
            self.library
                .forget_theme_in_church_settings(id)
                .map_err(err)
        } else {
            self.save_church(|c| {
                if c.default_theme == Some(id) {
                    c.default_theme = None;
                }
                if c.brand_theme == Some(id) {
                    c.brand_theme = None;
                }
            })
        }
    }

    /// Saves what the UI sent: this device's own settings in
    /// `settings.json`, the church's through `save_church`.
    pub(super) fn put_device_settings(&mut self, sent: DeviceSettings) -> Result<()> {
        let mut own = self.settings.get().clone();
        own.clicker_mode = sent.clicker_mode;
        own.use_sound = sent.use_sound;
        own.sound_output = sent.sound_output;
        self.audio.set_output(own.sound_output.clone());
        own.default_translation = sent.default_translation;
        if own != *self.settings.get() {
            self.settings.put(own).map_err(err)?;
        }
        // The UI sends back what it read, and a default theme it was not
        // shown (deleted) as unset: keep a document value it never saw.
        self.save_church(|c| {
            c.brand = sent.brand;
            c.brand_theme = sent.brand_theme;
            c.default_theme = sent.default_theme;
            c.idle = sent.idle;
        })
    }

    /// Files the shared settings point at: the logo, the idle picture,
    /// video or deck, and the pictures and videos of the default and
    /// brand themes.
    fn church_files(&self) -> Vec<(BlobHash, &'static str)> {
        let church = self.church();
        let mut doc = ChurchSettings::default();
        church.put_in_doc(&mut doc);
        let mut files = doc.files();
        if let Some(IdleScreen::DeckSlide { deck, .. }) = &church.idle {
            if let Ok(Some(deck)) = self.library.deck(*deck) {
                files.push((deck.current.blob, "idle slide"));
            }
        }
        for id in [church.default_theme, church.brand_theme]
            .into_iter()
            .flatten()
        {
            if let Ok(Some(theme)) = self.library.theme(id) {
                match theme.background {
                    Background::Image(blob) => files.push((blob, "theme picture")),
                    Background::Video(blob) => files.push((blob, "theme video")),
                    _ => {}
                }
            }
        }
        files.dedup_by_key(|(blob, _)| *blob);
        files
    }

    fn church_author(&self) -> String {
        self.library
            .church_settings()
            .ok()
            .and_then(|d| d.updated_by)
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| "your other devices".into())
    }

    /// Looks at the shared settings now and then: asks for files that have
    /// not arrived, shows the screens once they do, and makes the
    /// first-time agreement with a newly met device. Cheap, so the shell
    /// calls it with every tick.
    pub fn church_tick(&mut self) {
        self.church_tick_at(Instant::now());
    }

    /// `church_tick` with the time given, for tests.
    pub fn church_tick_at(&mut self, now: Instant) {
        if self
            .church_sync
            .looked
            .is_some_and(|at| now.duration_since(at) < LOOK)
        {
            return;
        }
        self.church_sync.looked = Some(now);
        self.church_agree(now);
        self.church_files_arrive(now);
    }

    /// Looks at the settings at the next tick whatever the last look was.
    pub(super) fn church_sync_look_now(&mut self) {
        self.church_sync.looked = None;
    }

    fn church_files_arrive(&mut self, now: Instant) {
        let waiting: Vec<_> = self
            .church_files()
            .into_iter()
            .filter(|(blob, _)| self.library.repo().blobs().path(blob).is_none())
            .collect();
        let arrived = self
            .church_sync
            .waiting
            .iter()
            .any(|w| !waiting.contains(w));
        let changed = waiting != self.church_sync.waiting;
        self.church_sync.waiting = waiting;
        if self
            .church_sync
            .asked_at
            .is_some_and(|at| now.duration_since(at) >= ASK_AGAIN)
        {
            self.church_sync.asked.clear();
            self.church_sync.asked_at = None;
        }
        if let Some(link) = self.church_sync.link.clone() {
            let ask: Vec<BlobHash> = self
                .church_sync
                .waiting
                .iter()
                .map(|(blob, _)| *blob)
                .filter(|blob| self.church_sync.asked.insert(*blob))
                .collect();
            if !ask.is_empty() && !link.connected().is_empty() {
                self.church_sync.asked_at.get_or_insert(now);
                link.fetch(ask);
            } else if !ask.is_empty() {
                // Nobody to ask yet: ask again when somebody is there.
                for blob in ask {
                    self.church_sync.asked.remove(&blob);
                }
            }
        }
        if arrived {
            // A file the screens were waiting for is here.
            self.sent.clear();
            self.present();
        }
        if changed || arrived {
            (self.emit)(lampstand_api::Event::Changed {
                kind: "church".into(),
                id: String::new(),
            });
        }
    }

    fn church_agree(&mut self, now: Instant) {
        if self.settings.get().church_synced {
            return;
        }
        let Some(link) = self.church_sync.link.clone() else {
            return;
        };
        let peers = link.connected();
        if peers.is_empty() {
            self.church_sync.settle_at = None;
            return;
        }
        let shared = Church::from_doc(&self.library.church_settings().unwrap_or_default());
        let settle_at = *self.church_sync.settle_at.get_or_insert_with(|| {
            let mine = link.device_key();
            now + if peers.iter().all(|p| mine < *p) {
                SETTLE
            } else {
                SETTLE_LATER
            }
        });
        if shared.is_empty() && now < settle_at {
            return;
        }
        let mine = Church::from_local(self.settings.get());
        match agree(&mine, &shared) {
            Agree::UseShared => self.church_adopt(None),
            Agree::OfferMine => self.church_adopt(Some(mine)),
            Agree::Ask => {
                let conflict = ChurchConflict {
                    other_device: self.church_author(),
                    mine: mine.summary(),
                    theirs: shared.summary(),
                };
                if self.church_sync.conflict.as_ref() != Some(&conflict) {
                    self.church_sync.conflict = Some(conflict);
                    (self.emit)(lampstand_api::Event::Changed {
                        kind: "church".into(),
                        id: String::new(),
                    });
                }
            }
        }
    }

    /// Agrees with the church: from now on the document holds the shared
    /// settings. With `offer`, this device's own are put in it first.
    fn church_adopt(&mut self, offer: Option<Church>) {
        let result = (|| -> Result<()> {
            if let Some(mine) = offer {
                let mut doc = self.library.church_settings().map_err(err)?;
                mine.put_in_doc(&mut doc);
                doc.updated_by = self.church_sync.link.as_ref().map(|l| l.device_name());
                self.library.put_church_settings(&doc).map_err(err)?;
            }
            let mut settings = self.settings.get().clone();
            settings.church_synced = true;
            settings.brand = None;
            settings.brand_theme = None;
            settings.default_theme = None;
            settings.idle = None;
            self.settings.put(settings).map_err(err)
        })();
        if let Err(e) = result {
            eprintln!("lampstand: sharing the church settings failed: {e}");
            return;
        }
        self.church_sync.conflict = None;
        self.sent.clear();
        self.present();
        (self.emit)(lampstand_api::Event::Changed {
            kind: "church".into(),
            id: String::new(),
        });
    }

    /// `SettingsRequest::ResolveChurch`.
    pub(super) fn resolve_church(&mut self, choice: ChurchChoice) -> Result<()> {
        if self.settings.get().church_synced || self.church_sync.conflict.is_none() {
            return Ok(());
        }
        let mine = Church::from_local(self.settings.get());
        self.church_adopt(match choice {
            ChurchChoice::ThisDevice => Some(mine),
            ChurchChoice::Shared => None,
        });
        Ok(())
    }

    /// `SettingsRequest::Church`.
    pub(super) fn church_status(&mut self) -> ChurchStatus {
        // What is waiting is current, not as of the last tick.
        self.church_sync.looked = None;
        self.church_tick();
        let from = self.church_author();
        ChurchStatus {
            shared: self.settings.get().church_synced,
            conflict: self.church_sync.conflict.clone(),
            waiting: self
                .church_sync
                .waiting
                .iter()
                .map(|(_, what)| WaitingFile {
                    what: (*what).into(),
                    from: from.clone(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_core::geom::Color;

    fn church(name: &str) -> Church {
        Church {
            brand: Some(Brand {
                name: name.into(),
                logo: None,
                primary: Color::rgb(1, 2, 3),
                text: None,
                accent: Color::rgb(4, 5, 6),
                font_family: String::new(),
                bug: Default::default(),
            }),
            ..Church::default()
        }
    }

    #[test]
    fn a_device_with_nothing_takes_the_churchs_settings() {
        assert_eq!(
            agree(&Church::default(), &church("Grace")),
            Agree::UseShared
        );
        assert_eq!(
            agree(&Church::default(), &Church::default()),
            Agree::UseShared
        );
    }

    #[test]
    fn a_device_with_settings_offers_them_when_the_church_has_none() {
        assert_eq!(
            agree(&church("Grace"), &Church::default()),
            Agree::OfferMine
        );
    }

    #[test]
    fn the_same_settings_agree_and_different_ones_ask() {
        assert_eq!(agree(&church("Grace"), &church("Grace")), Agree::UseShared);
        assert_eq!(agree(&church("Grace"), &church("Mercy")), Agree::Ask);
        let mut other = church("Grace");
        other.default_theme = Some(ThemeId::generate());
        assert_eq!(agree(&church("Grace"), &other), Agree::Ask);
    }

    #[test]
    fn a_summary_says_what_is_set() {
        let mut c = church("Grace Chapel");
        c.brand.as_mut().unwrap().logo = Some(BlobHash::of(b"x"));
        c.idle = Some(IdleScreen::Video {
            blob: BlobHash::of(b"v"),
            name: "Loop.mp4".into(),
        });
        assert_eq!(c.summary(), "Grace Chapel, logo, Looping video: Loop.mp4");
    }
}
