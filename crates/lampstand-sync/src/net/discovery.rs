//! mDNS: advertise `_lampstand._tcp` with this device's key, and watch for
//! other Lampstand devices.
//!
//! TXT properties: `id` (the device key in hex), `name`, `pairing` (`1`
//! while a pairing code is shown) and `v` (the wire protocol). Church
//! networks often block multicast between Wi-Fi and wired segments, so
//! connecting by address works without any of this.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Weak};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

use super::{lock, Found, SyncNode};
use crate::team::DeviceKey;

pub(crate) struct Discovery {
    daemon: ServiceDaemon,
    service: String,
    instance: String,
    host: String,
    port: u16,
    id: String,
    name: String,
}

impl Discovery {
    pub fn start(node: &Arc<SyncNode>, port: u16) -> Result<Self, mdns_sd::Error> {
        let daemon = ServiceDaemon::new()?;
        let id = node.device().to_hex();
        // Instance and host names unique per device, so two devices (or two
        // copies on one machine) never clash.
        let instance = format!("lampstand-{}", &id[..12]);
        let d = Self {
            service: lock(&node.service).clone(),
            host: format!("{instance}.local."),
            instance,
            daemon,
            port,
            id,
            name: node.name.clone(),
        };
        d.advertise(node.status().pairing.is_some());
        let events = d.daemon.browse(&d.service)?;
        let node: Weak<SyncNode> = Arc::downgrade(node);
        std::thread::Builder::new()
            .name("lampstand-mdns".into())
            .spawn(move || {
                while let Ok(event) = events.recv() {
                    let Some(node) = node.upgrade() else {
                        return;
                    };
                    match event {
                        ServiceEvent::ServiceResolved(info) => {
                            let Some(device) = info
                                .get_property_val_str("id")
                                .and_then(DeviceKey::from_hex)
                            else {
                                continue;
                            };
                            let mut addresses: Vec<SocketAddr> = info
                                .get_addresses_v4()
                                .into_iter()
                                .map(|ip| SocketAddr::new(IpAddr::V4(ip), info.port))
                                .collect();
                            addresses.sort_by_key(|a| (reach_order(a.ip()), *a));
                            node.found(
                                device,
                                Found {
                                    fullname: info.fullname.clone(),
                                    name: info
                                        .get_property_val_str("name")
                                        .unwrap_or("Lampstand")
                                        .to_string(),
                                    addresses,
                                    pairing: info.get_property_val_str("pairing") == Some("1"),
                                },
                            );
                        }
                        ServiceEvent::ServiceRemoved(_, fullname) => node.lost(&fullname),
                        _ => {}
                    }
                }
            })
            .map_err(|e| mdns_sd::Error::Msg(e.to_string()))?;
        Ok(d)
    }

    /// (Re-)registers this device, with the pairing flag as given.
    pub fn advertise(&self, pairing: bool) {
        let properties = [
            ("id", self.id.as_str()),
            ("name", self.name.as_str()),
            ("pairing", if pairing { "1" } else { "0" }),
            ("v", "1"),
        ];
        let info = ServiceInfo::new(
            &self.service,
            &self.instance,
            &self.host,
            "",
            self.port,
            &properties[..],
        )
        .map(ServiceInfo::enable_addr_auto);
        match info {
            Ok(info) => {
                if let Err(e) = self.daemon.register(info) {
                    eprintln!("lampstand: mDNS registration failed: {e}");
                }
            }
            Err(e) => eprintln!("lampstand: mDNS registration failed: {e}"),
        }
    }
}

/// The order to try a device's addresses in. A computer advertises every
/// address it has, so a laptop on Wi-Fi with a VPN offers both, and the
/// VPN's (Tailscale's 100.64/10, say) is no use to a computer that isn't on
/// it: the home network's addresses come first, then others, then the
/// shared-address range VPNs use and self-assigned 169.254 ones, and
/// loopback, which only works on the same machine, last.
fn reach_order(ip: IpAddr) -> u8 {
    let IpAddr::V4(v4) = ip else { return 3 };
    let [a, b, ..] = v4.octets();
    if v4.is_loopback() {
        5
    } else if v4.is_link_local() {
        4
    } else if a == 100 && (64..128).contains(&b) {
        3
    } else if v4.is_private() {
        0
    } else {
        1
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn the_home_network_is_tried_before_a_vpn_and_loopback_last() {
        let ip = |a, b, c, d| IpAddr::V4(Ipv4Addr::new(a, b, c, d));
        let mut found = vec![
            ip(127, 0, 0, 1),
            ip(100, 101, 7, 12),
            ip(169, 254, 3, 9),
            ip(192, 168, 1, 20),
            ip(203, 0, 113, 5),
            ip(10, 0, 0, 4),
        ];
        found.sort_by_key(|a| (reach_order(*a), *a));
        assert_eq!(
            found,
            [
                ip(10, 0, 0, 4),
                ip(192, 168, 1, 20),
                ip(203, 0, 113, 5),
                ip(100, 101, 7, 12),
                ip(169, 254, 3, 9),
                ip(127, 0, 0, 1),
            ]
        );
    }
}
