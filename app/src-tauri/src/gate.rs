//! The one gate between this device's live engine and anything that drives
//! it from elsewhere (decision 0006, item 4): a phone remote paired by PIN
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
use lampstand_live::Command;

/// Who sent a request from elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caller {
    /// A phone or tablet's browser, paired by PIN over plain HTTP.
    Remote,
    /// A paired Lampstand device driving this one.
    Driver,
}

/// Whether `caller` may have this device carry out `request`.
pub fn allows(caller: Caller, request: &Request) -> bool {
    match caller {
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
            Request::Live(Command::ToggleLogo),
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
