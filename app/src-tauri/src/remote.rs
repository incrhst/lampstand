//! Lampstand Remote (phones and tablets) on the app's side (T99, T101; decision 0006): what the
//! remote pages are told, how their requests reach the app, and the
//! `Remote` requests behind the Connect dialog and the devices screen.
//!
//! The local server ([`lampstand_server::remote`]) pairs phones and carries
//! the socket. Every request a phone sends comes here first and passes
//! [`crate::gate`] before anything carries it out; a refused one is logged
//! and answered with an error, and never reaches the app.
//!
//! A remote page sees what Present's own screen sees: `Event::Live`,
//! `Event::Loaded` (or `Event::Unloaded`) and `Event::Sound`, through the
//! same event sink, so a laptop driving another device passes on the
//! presenting device's state as it gets it.

use std::future::Future;
use std::io;
use std::net::IpAddr;
use std::path::Path;

use lampstand_api::{
    Event, RemoteAddress, RemotePinView, RemoteRequest, RemoteSessionView, RemotesView, Request,
    Response, ServiceRequest,
};
use lampstand_core::service::{ItemId, ServiceId};
use lampstand_server::{RemoteHub, RemoteKind};
use qrcode::render::svg;
use qrcode::QrCode;

use crate::gate::{self, Caller};

/// The widest slide picture a phone may ask for: about twice a phone's
/// width in CSS pixels, for a sharp picture on a high-density screen.
const MAX_THUMB: u32 = 960;

/// The paired phones and the PIN, kept in `remotes.json` beside
/// `paired.json` in `data_dir`.
pub fn open_hub(data_dir: &Path) -> io::Result<RemoteHub> {
    RemoteHub::open(data_dir.join("sync").join("remotes.json"))
}

/// Tells the remote pages about `event`, if it is one they show. A
/// speaker's phone ends with its service: when the service is closed, or
/// another is opened in its place.
pub fn bridge(hub: &RemoteHub, event: &Event) {
    match event {
        Event::Unloaded => end_speakers(hub, None),
        Event::Loaded(view) => end_speakers(hub, Some(view.service)),
        _ => {}
    }
    let kind = match event {
        Event::Live { .. } => "Live",
        // The last of these two is what a page needs when it connects.
        Event::Loaded(_) | Event::Unloaded => "Loaded",
        Event::Sound(_) => "Sound",
        _ => return,
    };
    match serde_json::to_string(event) {
        Ok(json) => hub.publish(kind, &json),
        Err(e) => eprintln!("lampstand: cannot tell the remotes: {e}"),
    }
}

/// Disconnects the speakers' phones paired for any service but `open` (all
/// of them when nothing is open, or the service was ended).
pub fn end_speakers(hub: &RemoteHub, open: Option<ServiceId>) {
    let open = open.map(|s| s.to_string());
    let gone = hub.end_speakers(|service| open.as_deref() != Some(service));
    if gone > 0 {
        eprintln!("lampstand: the service ended, so {gone} speaker's phone(s) stopped");
    }
}

/// Who a remote session is, for the gate: a speaker for its item, or an
/// operator's phone. `None` for a speaker whose item cannot be read.
pub fn caller(kind: &RemoteKind) -> Option<Caller> {
    match kind {
        RemoteKind::Operator => Some(Caller::Remote),
        RemoteKind::Speaker { item, .. } => item.parse::<ItemId>().ok().map(Caller::Speaker),
    }
}

fn error(message: &str) -> serde_json::Value {
    serde_json::to_value(Response::Error {
        message: message.into(),
    })
    .unwrap_or_default()
}

/// Answers a request from a remote page (JSON as the page sent it), from a
/// session paired as `kind`: the gate, then `dispatch`, which carries it out
/// as the operator's own screen would. A refused request is logged and
/// never dispatched. A speaker's live command that passes still has to pass
/// `gate::speaker_apply` where `dispatch` carries it out.
pub async fn answer<F, Fut>(
    kind: &RemoteKind,
    request: serde_json::Value,
    dispatch: F,
) -> serde_json::Value
where
    F: FnOnce(Caller, Request) -> Fut,
    Fut: Future<Output = Response>,
{
    let Some(who) = caller(kind) else {
        eprintln!("lampstand: refused a speaker's phone whose item is unreadable");
        return error("This phone is not set up right. Ask for a new PIN on the laptop.");
    };
    let mut request = match serde_json::from_value::<Request>(request) {
        Ok(request) => request,
        Err(e) => {
            eprintln!("lampstand: a remote sent an unreadable request: {e}");
            return error("This page is out of date or damaged. Reload the page and try again.");
        }
    };
    if !gate::allows(who, &request) {
        eprintln!("lampstand: refused a remote's request ({who:?}): {request:?}");
        return error(match who {
            Caller::Speaker(_) => "A speaker's phone can only change its own slides.",
            _ => "A remote can only run the slides and the sound.",
        });
    }
    if let Request::Service(ServiceRequest::SlideThumbnail { width, .. }) = &mut request {
        *width = (*width).clamp(16, MAX_THUMB);
    }
    serde_json::to_value(dispatch(who, request).await).unwrap_or_else(|e| error(&e.to_string()))
}

/// `Remote` requests, for the Connect dialog and the devices screen.
/// `port` is the local server's; `speaker_item` finds an item of the open
/// service for a speaker's PIN (its service and title).
pub fn handle(
    hub: &RemoteHub,
    request: RemoteRequest,
    port: u16,
    speaker_item: impl FnOnce(ItemId) -> Option<(ServiceId, String)>,
) -> Response {
    match request {
        RemoteRequest::Status => {}
        RemoteRequest::StartPairing => {
            hub.start_pairing();
        }
        RemoteRequest::StartSpeakerPairing { item } => {
            let Some((service, title)) = speaker_item(item) else {
                return Response::Error {
                    message: "That item is not in the open service, or has no slides.".into(),
                };
            };
            hub.start_pairing_as(RemoteKind::Speaker {
                service: service.to_string(),
                item: item.to_string(),
                title,
            });
        }
        RemoteRequest::StopPairing => hub.stop_pairing(),
        RemoteRequest::Revoke { id } => {
            if !hub.revoke(&id) {
                return Response::Error {
                    message: "That remote is not paired any more.".into(),
                };
            }
            eprintln!("lampstand: a remote was revoked");
        }
    }
    Response::Remotes(view(hub, port))
}

/// The remote as the screens show it. The addresses (and their QR codes)
/// are worked out only while a PIN is shown.
pub fn view(hub: &RemoteHub, port: u16) -> RemotesView {
    let pairing = hub.pairing();
    let addresses = if pairing.is_some() {
        urls(&addresses(), port)
            .into_iter()
            .map(|url| RemoteAddress {
                qr_svg: qr(&url),
                url,
            })
            .collect()
    } else {
        Vec::new()
    };
    let speaker = |kind: &RemoteKind| match kind {
        RemoteKind::Speaker { title, .. } => Some(title.clone()),
        RemoteKind::Operator => None,
    };
    RemotesView {
        pairing_speaker: pairing.as_ref().and_then(|p| speaker(&p.kind)),
        pairing: pairing.map(|p| RemotePinView {
            pin: p.pin,
            expires_ms: p.expires_ms,
        }),
        addresses,
        sessions: hub
            .sessions()
            .into_iter()
            .map(|s| RemoteSessionView {
                speaker: speaker(&s.kind),
                id: s.id,
                name: s.name,
                paired_ms: s.paired_ms,
                last_seen_ms: s.last_seen_ms,
                connected: s.connected > 0,
            })
            .collect(),
    }
}

/// This computer's IPv4 addresses a phone could reach: the one the default
/// route leaves from first (the likeliest to share the phone's Wi-Fi),
/// then the rest. Never loopback or link-local. Raw IPs, because Android
/// often cannot resolve `.local` names.
fn addresses() -> Vec<IpAddr> {
    let route = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("192.0.2.1:9")?;
            s.local_addr()
        })
        .map(|a| a.ip())
        .ok();
    let others = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .map(|i| i.ip());
    order(route, others)
}

fn usable(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => !(v4.is_loopback() || v4.is_link_local() || v4.is_unspecified()),
        IpAddr::V6(_) => false,
    }
}

fn order(route: Option<IpAddr>, others: impl Iterator<Item = IpAddr>) -> Vec<IpAddr> {
    let mut rest: Vec<IpAddr> = others.filter(usable).collect();
    rest.sort();
    rest.dedup();
    let mut out: Vec<IpAddr> = route.filter(usable).into_iter().collect();
    out.extend(rest.into_iter().filter(|ip| Some(*ip) != route));
    out
}

fn urls(ips: &[IpAddr], port: u16) -> Vec<String> {
    ips.iter()
        .map(|ip| format!("http://{ip}:{port}/remote"))
        .collect()
}

/// `text` as a QR code, an SVG without the XML declaration so it can sit
/// inside a page. The quiet zone is part of the picture.
fn qr(text: &str) -> String {
    match QrCode::new(text.as_bytes()) {
        Ok(code) => {
            let svg = code
                .render::<svg::Color>()
                .min_dimensions(200, 200)
                .dark_color(svg::Color("#000000"))
                .light_color(svg::Color("#ffffff"))
                .build();
            match svg.find("<svg") {
                Some(at) => svg[at..].to_string(),
                None => svg,
            }
        }
        Err(e) => {
            eprintln!("lampstand: cannot make a QR code: {e}");
            String::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    #[test]
    fn the_routed_address_comes_first_and_unreachable_ones_are_left_out() {
        let wifi = v4(192, 168, 1, 20);
        let ethernet = v4(10, 0, 0, 5);
        let found = order(
            Some(wifi),
            [
                v4(127, 0, 0, 1),
                wifi,
                v4(169, 254, 3, 4),
                ethernet,
                "fe80::1".parse().unwrap(),
                v4(172, 16, 0, 9),
            ]
            .into_iter(),
        );
        assert_eq!(found, [wifi, ethernet, v4(172, 16, 0, 9)]);
        assert_eq!(urls(&found[..1], 7341), ["http://192.168.1.20:7341/remote"]);
        // No route (a laptop on a network with no internet): the rest, in
        // order.
        assert_eq!(order(None, [ethernet, wifi].into_iter()), [ethernet, wifi]);
        assert!(order(Some(v4(127, 0, 0, 1)), std::iter::empty()).is_empty());
    }

    #[test]
    fn the_qr_code_is_an_svg_for_the_page() {
        let svg = qr("http://192.168.1.20:7340/remote");
        assert!(svg.starts_with("<svg"), "{}", &svg[..40]);
        assert!(svg.ends_with("</svg>"));
        assert!(!svg.contains("<?xml"));
    }

    #[test]
    fn the_dialog_gets_addresses_only_while_a_pin_is_shown() {
        let dir = tempfile::tempdir().unwrap();
        let hub = open_hub(dir.path()).unwrap();
        let idle = handle(&hub, RemoteRequest::Status, 7340, |_| None);
        let Response::Remotes(idle) = idle else {
            panic!()
        };
        assert!(idle.pairing.is_none() && idle.addresses.is_empty());
        let Response::Remotes(pairing) = handle(&hub, RemoteRequest::StartPairing, 7340, |_| None)
        else {
            panic!()
        };
        assert_eq!(pairing.pairing.unwrap().pin.len(), 6);
        for address in &pairing.addresses {
            assert!(address.url.ends_with(":7340/remote"), "{}", address.url);
        }
        let Response::Error { .. } = handle(
            &hub,
            RemoteRequest::Revoke {
                id: "nobody".into(),
            },
            7340,
            |_| None,
        ) else {
            panic!("revoking a remote that is not paired is an error")
        };
    }

    #[test]
    fn a_speakers_pin_is_for_an_item_of_the_open_service_and_ends_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let hub = open_hub(dir.path()).unwrap();
        let (service, item) = (ServiceId::generate(), ItemId::generate());
        let Response::Error { .. } = handle(
            &hub,
            RemoteRequest::StartSpeakerPairing { item },
            7340,
            |_| None,
        ) else {
            panic!("an item that is not open cannot be paired for")
        };
        let Response::Remotes(view) = handle(
            &hub,
            RemoteRequest::StartSpeakerPairing { item },
            7340,
            |i| (i == item).then(|| (service, "Sermon".to_string())),
        ) else {
            panic!()
        };
        assert_eq!(view.pairing_speaker.as_deref(), Some("Sermon"));
        let pin = view.pairing.unwrap().pin;
        let from = v4(192, 168, 1, 30);
        let token = hub.pair(from, &pin, "Pastor").unwrap().0;
        let (_, _, kind) = hub.session_kind(&token).unwrap();
        assert_eq!(caller(&kind), Some(Caller::Speaker(item)));
        assert_eq!(view_sessions(&hub), [Some("Sermon".to_string())]);
        // The same service reloaded keeps it; closing the service ends it.
        end_speakers(&hub, Some(service));
        assert!(hub.session_kind(&token).is_some());
        bridge(&hub, &Event::Unloaded);
        assert!(hub.session_kind(&token).is_none());
    }

    #[test]
    fn a_live_event_without_cleared_reads_as_not_cleared() {
        // As a 0.9.0 device sends it when it drives this one.
        let old = r#"{"type":"Live","data":{"live":null,"shown":null,"preview":null,"idle":true,"overlay":"None"}}"#;
        let Event::Live { cleared, .. } = serde_json::from_str::<Event>(old).unwrap() else {
            panic!()
        };
        assert!(!cleared);
    }

    fn view_sessions(hub: &RemoteHub) -> Vec<Option<String>> {
        view(hub, 7340)
            .sessions
            .into_iter()
            .map(|s| s.speaker)
            .collect()
    }
}
