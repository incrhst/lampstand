//! The one gate between this device's live engine and anything that drives
//! it from elsewhere (decision 0006, item 4): a remote paired by PIN
//! (T99) and a paired Lampstand device driving this one (T102, decision
//! 0009). Every request from either passes [`allows`] before it is carried
//! out, so what the two may do is decided in one place and cannot drift.
//!
//! A remote gets the least: the live commands that run the slides, the
//! sound in the service (decision 0006's amendment), and a picture of a slide
//! of the service on air. Nothing that edits, reads settings or the library,
//! or changes outputs. A driving device may do more, because it is paired
//! with an encrypted, authenticated link; what it may not do stays on the
//! device it belongs to.

use lampstand_api::{ObsRequest, Request, ServiceRequest, SoundRequest};
use lampstand_core::program::Overlay;
use lampstand_core::service::ItemId;
use lampstand_live::{Command, LiveEngine};

/// Who sent a request from elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caller {
    /// A phone or tablet's browser, paired by PIN over plain HTTP.
    Remote,
    /// A speaker's phone, paired by PIN for one item of the service
    /// (decision 0006, amended for 0.9.1): Next, Back and Blank through that
    /// item's slides while it is on air, and pictures of its slides.
    Speaker(ItemId),
    /// A paired Lampstand device driving this one.
    Driver,
}

/// Whether `caller` may have this device carry out `request`.
///
/// For a speaker this is only the first half: a live command that passes
/// here must also pass [`speaker_apply`], which checks it against the live
/// engine as it is at that moment and carries it out in the same step.
pub fn allows(caller: Caller, request: &Request) -> bool {
    match caller {
        Caller::Speaker(own) => match request {
            Request::Live(command) => {
                matches!(
                    command,
                    Command::Next | Command::Previous | Command::ToggleBlack
                )
            }
            // Pictures of its own slides, and no other item's.
            Request::Service(ServiceRequest::SlideThumbnail { item, .. }) => *item == own,
            _ => false,
        },
        Caller::Remote => {
            matches!(
                request,
                // Ending the service is the operator's, at the laptop.
                Request::Live(command) if !matches!(command, Command::EndService)
            ) || matches!(
                request,
                Request::Sound(
                SoundRequest::Play { .. }
                    | SoundRequest::Pause
                    | SoundRequest::Resume
                    | SoundRequest::Stop
                    | SoundRequest::Panic
            )
            // What the screens show or are about to: no more than the
            // room can see.
            | Request::Service(ServiceRequest::SlideThumbnail { .. })
            )
        }
        // T18: once teams exist, only an operator or admin passes here.
        Caller::Driver => matches!(
            request,
            Request::Live(_)
                | Request::Service(
                    ServiceRequest::Open(_)
                        | ServiceRequest::Close(_)
                        | ServiceRequest::SetLiveLoop { .. }
                )
                | Request::Obs(
                    ObsRequest::SetScene { .. } | ObsRequest::Resume | ObsRequest::SetMute { .. }
                )
                // Starting a sound needs the service the Projection device
                // has open; pause, stop and panic go from the driver itself,
                // so they work even when Projection is away.
                | Request::Sound(SoundRequest::Play { .. })
        ),
    }
}

/// Why a speaker's live command is refused, in the words its phone shows,
/// or `None` if the live engine as it is now lets it through: the speaker's
/// item is on air, the screens do not show the idle screen, its words have
/// not been cleared (Next and Back only; Blank stays allowed), and Next or
/// Back lands on a slide or build of that same item. Next on its last slide,
/// fully built, and Back on its first are refused, so a speaker can never
/// move the service on to another item.
pub fn speaker_refusal(own: ItemId, command: Command, live: &LiveEngine) -> Option<&'static str> {
    if live.live().map(|c| c.item) != Some(own) {
        return Some("Next and Back work only while your slides are on the screen.");
    }
    if live.program().overlay == Overlay::Idle {
        return Some("The service team has put up the idle screen.");
    }
    // Blank stays allowed: it never puts words back on the screen, and
    // taking Blank off again shows only what was there.
    if command == Command::ToggleBlack {
        return None;
    }
    // The operator cleared the words (Clear text, Clear all): Next or Back
    // would put a slide's words back on the projector.
    if live.program().content.is_none() {
        return Some("The service team has cleared the screen.");
    }
    match command {
        Command::Next if live.next_target().is_some_and(|t| t.item == own) => None,
        Command::Next => Some("That is the last slide. The service team takes it from here."),
        Command::Previous if live.previous_target().is_some_and(|t| t.item == own) => None,
        Command::Previous => Some("That is your first slide."),
        _ => Some("A speaker's phone can only change its own slides."),
    }
}

/// A speaker's live command: checked against the live engine and, if it
/// passes, applied, both while the caller holds `live` mutably. Nothing can
/// move the service between the check and the change (the app holds its one
/// lock throughout), so a Next pressed as the operator moves on to the next
/// item lands on that item's check and is refused, never applied to it.
pub fn speaker_apply(
    own: ItemId,
    command: Command,
    live: &mut LiveEngine,
) -> Result<Vec<lampstand_live::Event>, &'static str> {
    match speaker_refusal(own, command, live) {
        Some(why) => Err(why),
        None => Ok(live.apply(command)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_api::{
        LibraryRequest, OutputRequest, RemoteRequest, SettingsRequest, TeamRequest,
    };
    use lampstand_core::service::{ItemId, ServiceId};

    fn item() -> ItemId {
        ItemId::generate()
    }

    #[test]
    fn a_remote_may_run_the_slides_and_the_sound_and_nothing_else() {
        let allowed = [
            Request::Live(Command::Next),
            Request::Live(Command::Previous),
            Request::Live(Command::ToggleBlack),
            Request::Live(Command::ToggleIdle),
            Request::Live(Command::ClearAll),
            Request::Sound(SoundRequest::Play {
                item: item(),
                backing: false,
            }),
            Request::Sound(SoundRequest::Pause),
            Request::Sound(SoundRequest::Resume),
            Request::Sound(SoundRequest::Stop),
            Request::Sound(SoundRequest::Panic),
            Request::Service(ServiceRequest::SlideThumbnail {
                item: item(),
                slide: 0,
                step: 0,
                width: 480,
            }),
        ];
        for request in &allowed {
            assert!(allows(Caller::Remote, request), "{request:?}");
        }
        let refused = [
            Request::Live(Command::EndService),
            Request::Sound(SoundRequest::Outputs),
            Request::Sound(SoundRequest::Status),
            Request::Service(ServiceRequest::List),
            Request::Service(ServiceRequest::Open(ServiceId::generate())),
            Request::Service(ServiceRequest::Delete(ServiceId::generate())),
            Request::Library(LibraryRequest::ListThemes),
            Request::Settings(SettingsRequest::Get),
            Request::Outputs(OutputRequest::List),
            Request::Team(TeamRequest::Status),
            Request::Team(TeamRequest::StartPairing),
            Request::Obs(ObsRequest::Resume),
            Request::Remote(RemoteRequest::StartPairing),
            Request::Remote(RemoteRequest::Revoke { id: "x".into() }),
        ];
        for request in &refused {
            assert!(!allows(Caller::Remote, request), "{request:?}");
        }
    }

    mod speaker {
        use super::*;
        use lampstand_core::program::{Content, SlideText};
        use lampstand_live::{Cursor, LoadedItem, LoadedSlide};
        use std::sync::{Arc, Mutex};

        fn text(line: &str) -> LoadedSlide {
            LoadedSlide {
                content: Content::Text {
                    slide: SlideText {
                        label: None,
                        lines: vec![line.into()],
                        footer: None,
                    },
                },
                steps: 0,
                dwell_ms: None,
            }
        }

        fn built(line: &str, steps: usize) -> LoadedSlide {
            LoadedSlide {
                steps,
                ..text(line)
            }
        }

        fn item(slides: Vec<LoadedSlide>) -> LoadedItem {
            LoadedItem {
                id: ItemId::generate(),
                title: "item".into(),
                slides,
                background: None,
                theme: None,
                song: None,
                auto_advance_ms: None,
                loop_group: None,
                sound_only: false,
            }
        }

        /// A song, the speaker's talk (two slides, the second with one
        /// build), and a closing song.
        fn service() -> (LiveEngine, ItemId, ItemId, ItemId) {
            let (before, talk, after) = (
                item(vec![text("b1")]),
                item(vec![text("t1"), built("t2", 1)]),
                item(vec![text("a1")]),
            );
            let ids = (before.id, talk.id, after.id);
            let mut live = LiveEngine::new();
            live.load(vec![before, talk, after]);
            (live, ids.0, ids.1, ids.2)
        }

        fn at(item: ItemId, slide: usize, step: usize) -> Cursor {
            Cursor { item, slide, step }
        }

        #[test]
        fn a_speaker_may_send_next_back_and_blank_and_its_own_pictures() {
            let own = ItemId::generate();
            let caller = Caller::Speaker(own);
            for command in [Command::Next, Command::Previous, Command::ToggleBlack] {
                assert!(allows(caller, &Request::Live(command)), "{command:?}");
            }
            let picture = |item| {
                Request::Service(ServiceRequest::SlideThumbnail {
                    item,
                    slide: 0,
                    step: 0,
                    width: 240,
                })
            };
            assert!(allows(caller, &picture(own)));
            assert!(
                !allows(caller, &picture(ItemId::generate())),
                "another item's slide"
            );
            let refused = [
                Request::Live(Command::Show(at(own, 0, 0))),
                Request::Live(Command::ToggleIdle),
                Request::Live(Command::ClearText),
                Request::Live(Command::ClearAll),
                Request::Live(Command::NextItem),
                Request::Live(Command::EndService),
                Request::Live(Command::GoLive),
                Request::Sound(SoundRequest::Panic),
                Request::Sound(SoundRequest::Play {
                    item: own,
                    backing: false,
                }),
                Request::Service(ServiceRequest::List),
                Request::Remote(RemoteRequest::Status),
            ];
            for request in &refused {
                assert!(!allows(caller, request), "{request:?}");
            }
        }

        #[test]
        fn next_and_back_move_through_the_speakers_item_and_stop_at_both_ends() {
            let (mut live, _, talk, _) = service();
            live.apply(Command::Show(at(talk, 0, 0)));
            // Back on the first slide would leave the item.
            assert!(speaker_apply(talk, Command::Previous, &mut live).is_err());
            assert_eq!(live.live(), Some(at(talk, 0, 0)), "nothing moved");
            speaker_apply(talk, Command::Next, &mut live).unwrap();
            assert_eq!(live.live(), Some(at(talk, 1, 0)));
            speaker_apply(talk, Command::Next, &mut live).unwrap();
            assert_eq!(live.live(), Some(at(talk, 1, 1)), "the build");
            // Fully built on the last slide: Next would go to the next item.
            let refused = speaker_apply(talk, Command::Next, &mut live).unwrap_err();
            assert!(refused.contains("last slide"), "{refused}");
            assert_eq!(live.live(), Some(at(talk, 1, 1)));
            speaker_apply(talk, Command::Previous, &mut live).unwrap();
            assert_eq!(live.live(), Some(at(talk, 1, 0)));
            // Blank, and back.
            speaker_apply(talk, Command::ToggleBlack, &mut live).unwrap();
            assert_eq!(live.program().overlay, Overlay::Black);
            speaker_apply(talk, Command::ToggleBlack, &mut live).unwrap();
            assert_eq!(live.program().overlay, Overlay::None);
        }

        #[test]
        fn a_speaker_cannot_put_words_back_after_the_operator_clears_them() {
            for clear in [Command::ClearAll, Command::ClearText] {
                let (mut live, _, talk, _) = service();
                live.apply(Command::Show(at(talk, 0, 0)));
                live.apply(clear);
                assert!(live.program().content.is_none());
                for command in [Command::Next, Command::Previous] {
                    let why = speaker_apply(talk, command, &mut live).unwrap_err();
                    assert_eq!(why, "The service team has cleared the screen.");
                }
                assert_eq!(live.live(), Some(at(talk, 0, 0)), "nothing moved");
                assert!(live.program().content.is_none(), "still clear");
                // Blank stays allowed, and brings no words back.
                speaker_apply(talk, Command::ToggleBlack, &mut live).unwrap();
                speaker_apply(talk, Command::ToggleBlack, &mut live).unwrap();
                assert!(live.program().content.is_none());
                // The operator shows the talk again: the speaker carries on.
                live.apply(Command::Show(at(talk, 0, 0)));
                speaker_apply(talk, Command::Next, &mut live).unwrap();
                assert_eq!(live.live(), Some(at(talk, 1, 0)));
            }
        }

        #[test]
        fn a_speaker_can_do_nothing_while_another_item_or_the_idle_screen_is_up() {
            let (mut live, before, talk, _) = service();
            for command in [Command::Next, Command::Previous, Command::ToggleBlack] {
                assert!(
                    speaker_apply(talk, command, &mut live).is_err(),
                    "nothing live"
                );
            }
            live.apply(Command::Show(at(before, 0, 0)));
            // Next from the song before would land on the talk: still refused,
            // because the talk is not on air.
            for command in [Command::Next, Command::Previous, Command::ToggleBlack] {
                assert!(
                    speaker_apply(talk, command, &mut live).is_err(),
                    "{command:?}"
                );
            }
            assert_eq!(live.live(), Some(at(before, 0, 0)));
            live.apply(Command::Show(at(talk, 0, 0)));
            live.apply(Command::ToggleIdle);
            assert!(speaker_apply(talk, Command::Next, &mut live).is_err());
            assert_eq!(live.program().overlay, Overlay::Idle, "Next would lift it");
        }

        #[test]
        fn a_next_racing_the_operator_never_moves_the_service_on() {
            // The operator jumps between the talk's last slide and the next
            // item while the speaker presses Next as fast as it can. Every
            // Next the speaker gets through must start and end in the talk.
            let (mut live, _, talk, after) = service();
            live.apply(Command::Show(at(talk, 1, 1)));
            let live = Arc::new(Mutex::new(live));
            let operator = {
                let live = live.clone();
                std::thread::spawn(move || {
                    for n in 0..2000 {
                        let to = if n % 2 == 0 {
                            at(after, 0, 0)
                        } else {
                            at(talk, 0, 0)
                        };
                        live.lock().unwrap().apply(Command::Show(to));
                    }
                })
            };
            let mut through = 0;
            for _ in 0..2000 {
                let mut engine = live.lock().unwrap();
                let from = engine.live();
                if speaker_apply(talk, Command::Next, &mut engine).is_ok() {
                    through += 1;
                    assert_eq!(from.map(|c| c.item), Some(talk));
                    assert_eq!(engine.live().map(|c| c.item), Some(talk));
                } else {
                    assert_eq!(engine.live(), from, "a refused Next changes nothing");
                }
            }
            operator.join().unwrap();
            // The operator's last jump was to the talk's first slide.
            for _ in 0..5 {
                let mut engine = live.lock().unwrap();
                if speaker_apply(talk, Command::Next, &mut engine).is_ok() {
                    through += 1;
                }
            }
            let engine = live.lock().unwrap();
            assert_eq!(engine.live(), Some(at(talk, 1, 1)), "stopped at its end");
            assert!(through >= 2);
        }
    }

    #[test]
    fn a_driver_may_open_the_service_and_switch_obs_but_never_edit() {
        assert!(allows(Caller::Driver, &Request::Live(Command::EndService)));
        assert!(allows(
            Caller::Driver,
            &Request::Service(ServiceRequest::Open(ServiceId::generate()))
        ));
        assert!(allows(Caller::Driver, &Request::Obs(ObsRequest::Resume)));
        // Pause, stop and panic are sent by the driver itself.
        assert!(!allows(
            Caller::Driver,
            &Request::Sound(SoundRequest::Panic)
        ));
        assert!(!allows(
            Caller::Driver,
            &Request::Service(ServiceRequest::Delete(ServiceId::generate()))
        ));
        assert!(!allows(
            Caller::Driver,
            &Request::Settings(SettingsRequest::Get)
        ));
        assert!(!allows(
            Caller::Driver,
            &Request::Remote(RemoteRequest::Status)
        ));
    }
}
