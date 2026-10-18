//! CD-2: peer-to-peer sync on the church network, no server.
//!
//! Each device listens for sync WebSockets and advertises itself over mDNS as
//! `_lampstand._tcp` with its church id and peer id in the TXT record. When it
//! finds another device of the same church, the one with the lower peer id
//! dials, so each pair holds one connection. A peer still advertised after
//! its session drops is dialed again a few seconds later, so a dropped Wi-Fi
//! link heals by itself. Incoming connections must present the church token, so
//! another church on the same network cannot sync with us.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::net::TcpListener;
use tokio::task::{JoinHandle, JoinSet};

use crate::{client, server, session, ChurchCredentials, Hub, SyncError};

pub const SERVICE_TYPE: &str = "_lampstand._tcp.local.";
const REDIAL_EVERY: Duration = Duration::from_secs(5);

/// A device taking part in LAN sync. Dropping it stops listening,
/// advertising and every session it started.
pub struct LanPeer {
    hub: Arc<Hub>,
    creds: ChurchCredentials,
    local_addr: SocketAddr,
    mdns: Option<ServiceDaemon>,
    tasks: Vec<JoinHandle<()>>,
}

impl LanPeer {
    /// Listens on `bind` and, if `discovery` is true, advertises and browses
    /// over mDNS. Bind to `0.0.0.0:0` on a real network; any free port works
    /// because mDNS carries it.
    pub async fn start(
        hub: Arc<Hub>,
        creds: ChurchCredentials,
        bind: SocketAddr,
        discovery: bool,
    ) -> Result<Self, SyncError> {
        let listener = TcpListener::bind(bind).await?;
        let local_addr = listener.local_addr()?;
        let mut peer = Self {
            tasks: vec![tokio::spawn(accept_loop(
                listener,
                hub.clone(),
                creds.clone(),
            ))],
            hub,
            creds,
            local_addr,
            mdns: None,
        };
        if discovery {
            peer.start_discovery()?;
        }
        Ok(peer)
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Syncs with a peer at a known address, such as `ws://10.0.0.5:7878`,
    /// for networks where mDNS is blocked.
    pub fn connect(&mut self, base: String) {
        let (hub, creds) = (self.hub.clone(), self.creds.clone());
        self.tasks.push(tokio::spawn(async move {
            if let Err(e) = client::sync_with(hub, &base, &creds).await {
                tracing::warn!(peer = %base, error = %e, "lan session ended");
            }
        }));
    }

    fn start_discovery(&mut self) -> Result<(), SyncError> {
        let mdns = ServiceDaemon::new()?;
        let peer_id = self.hub.peer_id().to_owned();
        let host = format!("lampstand-{}.local.", &peer_id[..peer_id.len().min(12)]);
        let txt = [("church", self.creds.church()), ("peer", peer_id.as_str())];
        let info = ServiceInfo::new(
            SERVICE_TYPE,
            &peer_id,
            &host,
            (),
            self.local_addr.port(),
            &txt[..],
        )?
        .enable_addr_auto();
        mdns.register(info)?;
        let events = mdns.browse(SERVICE_TYPE)?;
        let known: Arc<Mutex<HashMap<String, Vec<SocketAddr>>>> = Arc::default();

        let (church, our_id) = (self.creds.church().to_owned(), peer_id);
        let browse_known = known.clone();
        self.tasks.push(tokio::spawn(async move {
            while let Ok(event) = events.recv_async().await {
                match event {
                    ServiceEvent::ServiceResolved(info) => {
                        let Some(their_id) = info.get_property_val_str("peer") else {
                            continue;
                        };
                        // Lower id dials, so each pair holds one connection.
                        if info.get_property_val_str("church") != Some(church.as_str())
                            || their_id <= our_id.as_str()
                        {
                            continue;
                        }
                        let mut addrs: Vec<SocketAddr> = info
                            .get_addresses_v4()
                            .into_iter()
                            .map(|ip| SocketAddr::new(IpAddr::V4(ip), info.get_port()))
                            .collect();
                        addrs.sort();
                        tracing::debug!(peer = their_id, ?addrs, "found lan peer");
                        browse_known
                            .lock()
                            .unwrap()
                            .insert(their_id.to_owned(), addrs);
                    }
                    ServiceEvent::ServiceRemoved(_, fullname) => {
                        let instance = fullname.split('.').next().unwrap_or_default();
                        browse_known.lock().unwrap().remove(instance);
                    }
                    _ => {}
                }
            }
        }));

        let (hub, creds) = (self.hub.clone(), self.creds.clone());
        self.tasks.push(tokio::spawn(dial_loop(hub, creds, known)));
        self.mdns = Some(mdns);
        Ok(())
    }
}

impl Drop for LanPeer {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        if let Some(mdns) = self.mdns.take() {
            let _ = mdns.shutdown();
        }
    }
}

async fn accept_loop(listener: TcpListener, hub: Arc<Hub>, creds: ChurchCredentials) {
    // Sessions live in the set so aborting this task ends them too.
    let mut sessions = JoinSet::new();
    loop {
        while sessions.try_join_next().is_some() {}
        let Ok((stream, from)) = listener.accept().await else {
            continue;
        };
        let (hub, creds) = (hub.clone(), creds.clone());
        sessions.spawn(async move {
            let authorize = |church: &str, token: &str| creds.matches(church, token).then_some(());
            let result = match server::accept(stream, authorize).await {
                Ok((ws, ())) => session::run(hub, ws).await,
                Err(e) => Err(e),
            };
            if let Err(e) = result {
                tracing::debug!(%from, error = %e, "lan session ended");
            }
        });
    }
}

/// Dials every known peer that has no live session. After a session ends or
/// every address fails, the peer waits [`REDIAL_EVERY`] before the next try.
async fn dial_loop(
    hub: Arc<Hub>,
    creds: ChurchCredentials,
    known: Arc<Mutex<HashMap<String, Vec<SocketAddr>>>>,
) {
    let busy: Arc<Mutex<HashSet<String>>> = Arc::default();
    let mut sessions = JoinSet::new();
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    loop {
        tick.tick().await;
        while sessions.try_join_next().is_some() {}
        let due: Vec<(String, Vec<SocketAddr>)> = {
            let known = known.lock().unwrap();
            let mut busy = busy.lock().unwrap();
            known
                .iter()
                .filter(|(id, _)| busy.insert((*id).clone()))
                .map(|(id, addrs)| (id.clone(), addrs.clone()))
                .collect()
        };
        for (id, addrs) in due {
            let (hub, creds, busy) = (hub.clone(), creds.clone(), busy.clone());
            sessions.spawn(async move {
                for addr in addrs {
                    match client::connect(&format!("ws://{addr}"), &creds).await {
                        Ok(ws) => {
                            tracing::info!(peer = %id, %addr, "lan peer connected");
                            if let Err(e) = session::run(hub, ws).await {
                                tracing::debug!(peer = %id, error = %e, "lan session ended");
                            }
                            break;
                        }
                        Err(e) => tracing::debug!(peer = %id, %addr, error = %e, "dial failed"),
                    }
                }
                tokio::time::sleep(REDIAL_EVERY).await;
                busy.lock().unwrap().remove(&id);
            });
        }
    }
}
