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
use lampstand_server::RemoteHub;
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

/// Tells the remote pages about `event`, if it is one they show.
pub fn bridge(hub: &RemoteHub, event: &Event) {
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

fn error(message: &str) -> serde_json::Value {
    serde_json::to_value(Response::Error {
        message: message.into(),
    })
    .unwrap_or_default()
}

/// Answers a request from a remote page (JSON as the page sent it): the
/// gate, then `dispatch`, which carries it out as the operator's own screen
/// would. A refused request is logged and never dispatched.
pub async fn answer<F, Fut>(request: serde_json::Value, dispatch: F) -> serde_json::Value
where
    F: FnOnce(Request) -> Fut,
    Fut: Future<Output = Response>,
{
    let mut request = match serde_json::from_value::<Request>(request) {
        Ok(request) => request,
        Err(e) => {
            eprintln!("lampstand: a remote sent an unreadable request: {e}");
            return error("This page is out of date or damaged. Reload the page and try again.");
        }
    };
    if !gate::allows(Caller::Remote, &request) {
        eprintln!("lampstand: refused a remote's request: {request:?}");
        return error("A remote can only run the slides and the sound.");
    }
    if let Request::Service(ServiceRequest::SlideThumbnail { width, .. }) = &mut request {
        *width = (*width).clamp(16, MAX_THUMB);
    }
    serde_json::to_value(dispatch(request).await).unwrap_or_else(|e| error(&e.to_string()))
}

/// `Remote` requests, for the Connect dialog and the devices screen.
/// `port` is the local server's.
pub fn handle(hub: &RemoteHub, request: RemoteRequest, port: u16) -> Response {
    match request {
        RemoteRequest::Status => {}
        RemoteRequest::StartPairing => {
            hub.start_pairing();
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
    RemotesView {
        pairing: pairing.map(|p| RemotePinView {
            pin: p.pin,
            expires_ms: p.expires_ms,
        }),
        addresses,
        sessions: hub
            .sessions()
            .into_iter()
            .map(|s| RemoteSessionView {
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
        let idle = handle(&hub, RemoteRequest::Status, 7340);
        let Response::Remotes(idle) = idle else {
            panic!()
        };
        assert!(idle.pairing.is_none() && idle.addresses.is_empty());
        let Response::Remotes(pairing) = handle(&hub, RemoteRequest::StartPairing, 7340) else {
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
        ) else {
            panic!("revoking a remote that is not paired is an error")
        };
    }
}
