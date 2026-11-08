//! A service order: the songs, scripture, decks and media for one service.
//!
//! PRD: PL-1, PS-8.
//!
//! Items live in a map keyed by ID, each with a fractional `Position`,
//! rather than in a list. Two planners reordering the same service offline
//! then merge cleanly: a CRDT list has no "move", so a move is a delete
//! plus an insert and concurrent moves duplicate the item.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::deck::DeckId;
use crate::id::{BlobHash, Id, Timestamp};
use crate::scripture::{Passage, TranslationId};
use crate::song::SongId;
use crate::theme::ThemeId;

pub type ServiceId = Id<Service>;
pub type ItemId = Id<ServiceItem>;

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub struct Service {
    pub id: ServiceId,
    pub title: String,
    pub starts_at: Option<Timestamp>,
    /// The IANA time zone the service is planned in ("Europe/London"), so
    /// `starts_at` reads as the church's wall-clock time wherever it is
    /// viewed. `None` reads it in the viewer's own zone. Documents saved
    /// before this field existed lack the key and read as `None`.
    #[cfg_attr(feature = "automerge", autosurgeon(missing = "Option::default"))]
    pub time_zone: Option<String>,
    /// Default theme for text items; each output may override it.
    pub theme: Option<ThemeId>,
    /// Keyed by the item ID as a string in the synced document.
    #[cfg_attr(
        feature = "automerge",
        autosurgeon(with = "autosurgeon::map_with_parseable_keys")
    )]
    pub items: BTreeMap<ItemId, ServiceItem>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// `Reconcile` is written by hand below, so that an item with no backing
// track writes no `backing` key (a derived `None` writes a null).
#[cfg_attr(feature = "automerge", derive(autosurgeon::Hydrate))]
pub struct ServiceItem {
    pub position: Position,
    pub kind: ItemKind,
    /// Planner's notes for the operator; never projected.
    pub notes: String,
    /// The OBS scene to switch to when this item goes live (OB-4, T90),
    /// by name; `None` leaves OBS as it is. Items saved before this field
    /// existed lack the key and read as `None`.
    #[serde(default)]
    #[cfg_attr(feature = "automerge", autosurgeon(missing = "Option::default"))]
    pub obs_scene: Option<String>,
    /// Loop, for pre- and post-service: seconds each slide stays up. On a
    /// header it loops that section (the items up to the next header); on
    /// any other item it loops that item's own slides. `None` is no loop.
    /// Items saved before this field existed read as `None`.
    #[serde(default)]
    #[cfg_attr(feature = "automerge", autosurgeon(missing = "Option::default"))]
    pub loop_seconds: Option<u16>,
    /// A song's backing track, which plays with the song (decision 0010).
    /// Stored on the item, not as an item of its own, so a song and its
    /// track cannot be pulled apart by a reorder. Items saved before sound
    /// existed lack the key and read as `None`; an item without one writes
    /// no key at all, so a service with no sound is the document it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    #[cfg_attr(feature = "automerge", autosurgeon(missing = "Option::default"))]
    pub backing: Option<Sound>,
}

/// Sound in the order of the service: a music-only item (walk-in music, a
/// sound cue) or a song's backing track. The audio file is a blob and
/// travels by sync; the cue only names it. Playing it is the Audio role's
/// job and is not decided here (decision 0010).
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub struct Sound {
    /// Shown in the order list: "Walk-in music", "Amazing Grace (track)".
    pub name: String,
    /// The audio file, in the blob store.
    pub file: BlobHash,
    /// How long it plays, in milliseconds, as read from the file when it
    /// was added. `None` when the file did not say.
    pub length_ms: Option<u32>,
    pub start: SoundStart,
    /// "Don't send this sound to the stream" (for copyright, or because it
    /// reaches the stream another way). Off unless set; the house is
    /// unaffected. Sending sound to the stream is a later ticket.
    pub keep_off_stream: bool,
}

/// How a sound starts when its item goes live.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub enum SoundStart {
    /// Plays by itself when the item goes live.
    OnItsOwn,
    /// Waits for somebody to press play.
    Waits,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub enum ItemKind {
    /// A divider in the order ("Worship", "Sermon"). Has no slides.
    Header {
        title: String,
    },
    Song {
        song: SongId,
        /// This service's arrangement, if it differs from the song's.
        arrangement: Option<Vec<String>>,
    },
    Scripture {
        passage: Passage,
        translation: TranslationId,
    },
    Deck {
        deck: DeckId,
    },
    Media {
        blob: BlobHash,
        name: String,
        looped: bool,
    },
    Countdown {
        target: CountdownTarget,
        label: String,
    },
    /// Text the planner types straight in: a quote, a welcome, prayer
    /// points. See [`crate::text::text_slides`] for how `body` splits.
    Text {
        /// Shown in the order list; never projected.
        title: String,
        body: String,
        /// A line at the foot of every slide, for a quote's author.
        attribution: Option<String>,
    },
    /// Images shown in a loop, for example before the service.
    Announcements {
        slides: Vec<BlobHash>,
        seconds_each: u16,
    },
    /// Music with no picture: walk-in music, a sound cue. It has no slides;
    /// the screens stay as they are when it goes live.
    Sound {
        sound: Sound,
    },
}

/// Writes the item field by field as the derive does, except `backing`: an
/// item without one leaves the key out, and drops it when the track is taken
/// off. A service with no sound is so the document it was, and opening an
/// old one writes nothing.
#[cfg(feature = "automerge")]
impl autosurgeon::Reconcile for ServiceItem {
    type Key<'a> = autosurgeon::reconcile::NoKey;

    fn reconcile<R: autosurgeon::Reconciler>(&self, mut reconciler: R) -> Result<(), R::Error> {
        use autosurgeon::reconcile::MapReconciler;
        let mut m = reconciler.map()?;
        m.put("position", &self.position)?;
        m.put("kind", &self.kind)?;
        m.put("notes", &self.notes)?;
        m.put("obs_scene", &self.obs_scene)?;
        m.put("loop_seconds", self.loop_seconds)?;
        match &self.backing {
            Some(sound) => m.put("backing", sound)?,
            None if m.entry("backing").is_some() => m.delete("backing")?,
            None => {}
        }
        Ok(())
    }
}

impl ServiceItem {
    /// The sound this item brings: its own, if it is a sound item, and a
    /// song's backing track.
    pub fn sounds(&self) -> impl Iterator<Item = &Sound> {
        let own = match &self.kind {
            ItemKind::Sound { sound } => Some(sound),
            _ => None,
        };
        own.into_iter().chain(self.backing.iter())
    }
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub enum CountdownTarget {
    /// Counts down to a wall-clock time (the service start).
    At(Timestamp),
    /// Counts down this many seconds from going live.
    Seconds(u32),
}

/// Fractional index: items sort by position string, then by ID when two
/// devices picked the same position concurrently. Positions only contain
/// the characters `0-9A-Za-z`, so they sort the same as bytes everywhere.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub struct Position(String);

const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Base-62 digit values of a position string (non-alphabet bytes count as 0).
fn digits(s: &str) -> Vec<u8> {
    s.bytes()
        .map(|c| ALPHABET.iter().position(|&a| a == c).unwrap_or(0) as u8)
        .collect()
}

/// A digit string strictly between `a` and `b`, read as base-62 fractions
/// (`a` shorter than `b` is padded with zeros). `None` means no upper bound.
/// The result never ends in a zero digit, so there is always room before it.
///
/// Where there is room it steps one digit away from the upper bound, or one
/// past the lower bound when appending. That lets both "insert before the
/// last one inserted" and "insert after the last one inserted" run about 60
/// times per added character; a plain midpoint would add one per 6.
fn midpoint(a: &[u8], b: Option<&[u8]>, deep: bool) -> Vec<u8> {
    let Some(b) = b else {
        return match a.first() {
            None if deep => vec![61],
            None => vec![31],
            Some(&61) if a.len() == 1 => vec![61, 1],
            Some(&61) => [&[61][..], &midpoint(&a[1..], None, true)].concat(),
            Some(&d) => vec![d + 1],
        };
    };
    let shared = (0..b.len())
        .take_while(|&i| a.get(i).copied().unwrap_or(0) == b[i])
        .count();
    if shared == b.len() {
        // `a` is not below `b`; callers filter this out.
        return midpoint(a, None, deep);
    }
    if shared > 0 {
        let rest = a.get(shared..).unwrap_or(&[]);
        let tail = midpoint(rest, Some(&b[shared..]), true);
        return [&b[..shared], &tail].concat();
    }
    let da = a.first().copied().unwrap_or(0);
    let db = b[0];
    if db - da > 1 {
        vec![db - 1]
    } else if b.len() > 1 {
        vec![db]
    } else {
        let tail = midpoint(a.get(1..).unwrap_or(&[]), None, true);
        [&[da][..], &tail].concat()
    }
}

impl Position {
    /// A position strictly between two others (either may be open). Keeps
    /// strings short: repeated inserts at the same spot grow them by about
    /// one character per 60 inserts.
    pub fn between(before: Option<&Position>, after: Option<&Position>) -> Position {
        let a: Vec<u8> = before.map_or_else(Vec::new, |p| digits(&p.0));
        let b: Option<Vec<u8>> = after.map(|p| digits(&p.0));
        let b = b.filter(|b| b.as_slice() > a.as_slice());
        let out = midpoint(&a, b.as_deref(), false);
        Position(
            out.into_iter()
                .map(|d| ALPHABET[d as usize] as char)
                .collect(),
        )
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Service {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            id: ServiceId::generate(),
            title: title.into(),
            starts_at: None,
            time_zone: None,
            theme: None,
            items: BTreeMap::new(),
        }
    }

    /// The items in running order.
    pub fn ordered(&self) -> Vec<(ItemId, &ServiceItem)> {
        let mut items: Vec<_> = self.items.iter().map(|(id, item)| (*id, item)).collect();
        items.sort_by(|(a_id, a), (b_id, b)| a.position.cmp(&b.position).then(a_id.cmp(b_id)));
        items
    }

    /// Inserts a new item so that it lands at `index` in running order
    /// (clamped to the end).
    pub fn insert(&mut self, index: usize, kind: ItemKind) -> ItemId {
        let ordered = self.ordered();
        let index = index.min(ordered.len());
        let before = index.checked_sub(1).map(|i| &ordered[i].1.position);
        let after = ordered.get(index).map(|(_, item)| &item.position);
        let position = Position::between(before, after);
        let id = ItemId::generate();
        let item = ServiceItem {
            position,
            kind,
            notes: String::new(),
            obs_scene: None,
            loop_seconds: None,
            backing: None,
        };
        self.items.insert(id, item);
        id
    }

    /// Moves an item to `index` in running order, changing only its own
    /// position. Returns false if there is no such item.
    pub fn move_item(&mut self, id: ItemId, index: usize) -> bool {
        if !self.items.contains_key(&id) {
            return false;
        }
        let others: Vec<&Position> = self
            .ordered()
            .into_iter()
            .filter(|(other, _)| *other != id)
            .map(|(_, item)| &item.position)
            .collect();
        let index = index.min(others.len());
        let before = index.checked_sub(1).map(|i| others[i]);
        let after = others.get(index).copied();
        let position = Position::between(before, after);
        if let Some(item) = self.items.get_mut(&id) {
            item.position = position;
        }
        true
    }

    /// How long the sound in the order plays, in milliseconds, and whether
    /// that is all of it: `false` when some file did not give its length.
    /// Music-only items follow one another, so they add up. A backing track
    /// plays with its song, which is not timed, so it adds nothing here.
    pub fn sound_ms(&self) -> (u64, bool) {
        let mut total = 0u64;
        let mut known = true;
        for item in self.items.values() {
            if let ItemKind::Sound { sound } = &item.kind {
                match sound.length_ms {
                    Some(ms) => total += u64::from(ms),
                    None => known = false,
                }
            }
        }
        (total, known)
    }

    /// Whether anything in the order is, or carries, sound.
    pub fn has_sound(&self) -> bool {
        self.items.values().any(|i| i.sounds().next().is_some())
    }

    pub fn remove(&mut self, id: ItemId) -> Option<ServiceItem> {
        self.items.remove(&id)
    }

    /// A new service started from this one: the same items in the same
    /// order, each with its notes, OBS scene, loop and arrangement, under
    /// `title` and a fresh ID. Every item gets a fresh ID too, so changing
    /// or deleting one in the copy never touches the original. Positions
    /// carry over, which keeps the order. The schedule is not copied.
    pub fn duplicate(&self, title: impl Into<String>) -> Service {
        let mut copy = Service::new(title);
        copy.theme = self.theme;
        copy.items = self
            .items
            .values()
            .map(|item| (ItemId::generate(), item.clone()))
            .collect();
        copy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(title: &str) -> ItemKind {
        ItemKind::Header {
            title: title.into(),
        }
    }

    fn titles(service: &Service) -> Vec<String> {
        service
            .ordered()
            .into_iter()
            .map(|(_, item)| match &item.kind {
                ItemKind::Header { title } => title.clone(),
                _ => unreachable!(),
            })
            .collect()
    }

    #[test]
    fn positions_sort_between_neighbours() {
        let a = Position::between(None, None);
        let b = Position::between(Some(&a), None);
        let mid = Position::between(Some(&a), Some(&b));
        let first = Position::between(None, Some(&a));
        assert!(first < a && a < mid && mid < b);
    }

    #[test]
    fn many_inserts_at_one_spot_stay_ordered_and_short() {
        let low = Position::between(None, None);
        let mut high = Position::between(Some(&low), None);
        for _ in 0..1000 {
            let p = Position::between(Some(&low), Some(&high));
            assert!(low < p && p < high);
            high = p;
        }
        assert!(high.as_str().len() < 40, "{}", high.as_str());
        assert!(high.as_str().chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn insert_and_move_keep_running_order() {
        let mut s = Service::new("Sunday");
        s.insert(0, header("B"));
        s.insert(0, header("A"));
        s.insert(9, header("D"));
        let c = s.insert(2, header("C"));
        assert_eq!(titles(&s), ["A", "B", "C", "D"]);
        assert!(s.move_item(c, 0));
        assert_eq!(titles(&s), ["C", "A", "B", "D"]);
        assert!(s.move_item(c, 3));
        assert_eq!(titles(&s), ["A", "B", "D", "C"]);
    }

    #[test]
    fn equal_positions_break_ties_by_id() {
        let mut s = Service::new("Sunday");
        let p = Position("U".into());
        for title in ["x", "y"] {
            let item = ServiceItem {
                position: p.clone(),
                kind: header(title),
                notes: String::new(),
                obs_scene: None,
                loop_seconds: None,
                backing: None,
            };
            s.items.insert(ItemId::generate(), item);
        }
        let ids: Vec<ItemId> = s.ordered().into_iter().map(|(id, _)| id).collect();
        assert!(ids[0] < ids[1]);
    }

    #[test]
    fn positions_with_no_neighbours_or_at_the_ends() {
        let mut last = Position::between(None, None);
        for _ in 0..200 {
            let next = Position::between(Some(&last), None);
            assert!(last < next);
            last = next;
        }
        assert!(last.as_str().len() < 6, "{}", last.as_str());
        let mut first = Position::between(None, None);
        for _ in 0..200 {
            let prev = Position::between(None, Some(&first));
            assert!(prev < first);
            first = prev;
        }
        assert!(first.as_str().len() < 6, "{}", first.as_str());
    }

    #[test]
    fn duplicate_keeps_order_and_fields_with_fresh_ids() {
        let mut s = Service::new("Sunday");
        let a = s.insert(0, header("A"));
        let b = s.insert(
            1,
            ItemKind::Song {
                song: SongId::generate(),
                arrangement: Some(vec!["Verse 1".into(), "Chorus".into()]),
            },
        );
        let c = s.insert(2, header("C"));
        s.items.get_mut(&b).unwrap().notes = "capo 2".into();
        s.items.get_mut(&b).unwrap().obs_scene = Some("Wide".into());
        s.items.get_mut(&a).unwrap().loop_seconds = Some(8);
        s.starts_at = Some(Timestamp(1_000));

        let mut copy = s.duplicate("Sunday (copy)");
        assert_eq!(copy.title, "Sunday (copy)");
        assert_ne!(copy.id, s.id);
        assert_eq!(copy.starts_at, None);

        let kept = |svc: &Service| -> Vec<ServiceItem> {
            svc.ordered().into_iter().map(|(_, i)| i.clone()).collect()
        };
        assert_eq!(kept(&copy), kept(&s));
        assert!(copy.items.keys().all(|id| ![a, b, c].contains(id)));

        // Independent: editing or deleting in the copy leaves the original.
        let before = s.clone();
        let (first, _) = copy.ordered()[1];
        copy.items.get_mut(&first).unwrap().notes = "changed".into();
        let (last, _) = copy.ordered()[2];
        copy.remove(last);
        assert_eq!(s, before);
        assert_eq!(copy.items.len(), 2);
    }

    fn sound(name: &str, ms: Option<u32>) -> Sound {
        Sound {
            name: name.into(),
            file: BlobHash::of(name.as_bytes()),
            length_ms: ms,
            start: SoundStart::Waits,
            keep_off_stream: false,
        }
    }

    #[test]
    fn music_only_items_add_up_and_a_backing_track_adds_nothing() {
        let mut s = Service::new("Sunday");
        assert_eq!(s.sound_ms(), (0, true));
        assert!(!s.has_sound());
        s.insert(
            0,
            ItemKind::Sound {
                sound: sound("Walk-in", Some(185_000)),
            },
        );
        s.insert(
            1,
            ItemKind::Sound {
                sound: sound("Offering", Some(60_000)),
            },
        );
        let song = s.insert(
            2,
            ItemKind::Song {
                song: SongId::generate(),
                arrangement: None,
            },
        );
        s.items.get_mut(&song).unwrap().backing = Some(sound("Track", Some(240_000)));
        assert_eq!(s.sound_ms(), (245_000, true));
        assert!(s.has_sound());
        // A file that gave no length makes the total a minimum.
        s.insert(
            3,
            ItemKind::Sound {
                sound: sound("Unknown", None),
            },
        );
        assert_eq!(s.sound_ms(), (245_000, false));
    }

    #[test]
    fn an_item_lists_its_own_sound_and_its_backing_track() {
        let mut s = Service::new("Sunday");
        let own = s.insert(
            0,
            ItemKind::Sound {
                sound: sound("Walk-in", None),
            },
        );
        let song = s.insert(
            1,
            ItemKind::Song {
                song: SongId::generate(),
                arrangement: None,
            },
        );
        let plain = s.insert(2, header("H"));
        s.items.get_mut(&song).unwrap().backing = Some(sound("Track", None));
        let names =
            |id: ItemId| -> Vec<String> { s.items[&id].sounds().map(|x| x.name.clone()).collect() };
        assert_eq!(names(own), ["Walk-in"]);
        assert_eq!(names(song), ["Track"]);
        assert!(names(plain).is_empty());
    }

    #[test]
    fn duplicate_keeps_sound_and_backing_tracks() {
        let mut s = Service::new("Sunday");
        s.insert(
            0,
            ItemKind::Sound {
                sound: sound("Walk-in", Some(1000)),
            },
        );
        let song = s.insert(
            1,
            ItemKind::Song {
                song: SongId::generate(),
                arrangement: None,
            },
        );
        s.items.get_mut(&song).unwrap().backing = Some(sound("Track", Some(2000)));
        let copy = s.duplicate("Next week");
        let kept = |svc: &Service| -> Vec<ServiceItem> {
            svc.ordered().into_iter().map(|(_, i)| i.clone()).collect()
        };
        assert_eq!(kept(&copy), kept(&s));
    }

    /// A service with no sound reads and writes as it did: the item has no
    /// `backing` key in JSON (the API and backups), and JSON written before
    /// sound existed reads back.
    #[test]
    fn an_item_without_sound_serialises_as_before() {
        let mut s = Service::new("Sunday");
        let id = s.insert(0, header("Welcome"));
        let json = serde_json::to_value(&s.items[&id]).unwrap();
        assert!(json.get("backing").is_none(), "{json}");
        let mut keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            ["kind", "loop_seconds", "notes", "obs_scene", "position"]
        );
        let old = serde_json::json!({
            "position": s.items[&id].position.as_str(), "kind": {"Header": {"title": "Welcome"}},
            "notes": "", "obs_scene": null, "loop_seconds": null
        });
        let read: ServiceItem = serde_json::from_value(old).unwrap();
        assert_eq!(read, s.items[&id]);
    }

    #[test]
    fn move_unknown_item_is_false() {
        let mut s = Service::new("Sunday");
        assert!(!s.move_item(ItemId::generate(), 0));
    }

    mod ordering_props {
        use super::*;
        use proptest::prelude::*;

        #[derive(Debug, Clone)]
        enum Op {
            Insert(usize),
            Move(usize, usize),
        }

        fn ops() -> impl Strategy<Value = Vec<Op>> {
            let op = prop_oneof![
                (0..40usize).prop_map(Op::Insert),
                (0..40usize, 0..40usize).prop_map(|(a, b)| Op::Move(a, b)),
            ];
            prop::collection::vec(op, 1..150)
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(64))]

            #[test]
            fn random_inserts_and_moves_match_a_list_model(ops in ops()) {
                let mut s = Service::new("Sunday");
                let mut model: Vec<ItemId> = Vec::new();
                for op in ops {
                    match op {
                        Op::Insert(i) => {
                            let id = s.insert(i, header("x"));
                            model.insert(i.min(model.len()), id);
                        }
                        Op::Move(from, to) if !model.is_empty() => {
                            let id = model.remove(from % model.len());
                            prop_assert!(s.move_item(id, to));
                            model.insert(to.min(model.len()), id);
                        }
                        Op::Move(..) => {}
                    }
                    let actual: Vec<ItemId> = s.ordered().into_iter().map(|(id, _)| id).collect();
                    prop_assert_eq!(&actual, &model);
                }
                for item in s.items.values() {
                    let p = item.position.as_str();
                    prop_assert!(p.chars().all(|c| c.is_ascii_alphanumeric()));
                    prop_assert!(!p.ends_with('0'));
                }
            }
        }
    }
}
