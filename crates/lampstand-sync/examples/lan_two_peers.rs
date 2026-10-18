//! CD-2 demo: two peers on this machine find each other over mDNS and sync,
//! with no server and no addresses configured.
//!
//!     cargo run -p lampstand-sync --example lan_two_peers
//!
//! Each peer has its own store in a temporary directory. The "office" peer
//! plans a service, the "laptop" peer receives it over the LAN, adds a song,
//! and the office sees the song.

use std::time::{Duration, Instant};

use lampstand_sync::automerge::transaction::Transactable;
use lampstand_sync::automerge::{ObjType, ReadDoc, ROOT};
use lampstand_sync::{ChurchCredentials, DocId, Hub, LanPeer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = DocId::new("services/demo")?;
    let creds = ChurchCredentials::generate("demo-church")?;
    let (office_dir, laptop_dir) = (tempfile::tempdir()?, tempfile::tempdir()?);
    let office = Hub::open(office_dir.path())?;
    let laptop = Hub::open(laptop_dir.path())?;

    office.create(&service, |doc| {
        doc.put_object(ROOT, "items", ObjType::List).map(drop)
    })?;
    office.edit(&service, "add Welcome", |doc| {
        let (_, items) = doc.get(ROOT, "items")?.expect("items");
        doc.insert(&items, 0, "Welcome")
    })?;

    let any = "0.0.0.0:0".parse()?;
    let _office_peer = LanPeer::start(office.clone(), creds.clone(), any, true).await?;
    let _laptop_peer = LanPeer::start(laptop.clone(), creds.clone(), any, true).await?;
    println!(
        "office  {} listening on {}",
        office.peer_id(),
        _office_peer.local_addr()
    );
    println!(
        "laptop  {} listening on {}",
        laptop.peer_id(),
        _laptop_peer.local_addr()
    );
    println!("waiting for mDNS discovery...");

    let started = Instant::now();
    wait(&laptop, &service, 1).await?;
    println!("laptop received the plan after {:?}", started.elapsed());

    laptop.edit(&service, "add Amazing Grace", |doc| {
        let (_, items) = doc.get(ROOT, "items")?.expect("items");
        doc.insert(&items, 1, "Amazing Grace")
    })?;
    wait(&office, &service, 2).await?;
    println!(
        "office sees the laptop's song after {:?}",
        started.elapsed()
    );

    let office_items = list(&office, &service);
    assert_eq!(office_items, list(&laptop, &service));
    println!("converged: {office_items:?}");
    Ok(())
}

fn list(hub: &Hub, id: &DocId) -> Vec<String> {
    let mut store = hub.store();
    let Some(doc) = store.doc(id) else {
        return vec![];
    };
    let Ok(Some((_, items))) = doc.get(ROOT, "items") else {
        return vec![];
    };
    (0..doc.length(&items))
        .filter_map(|i| doc.get(&items, i).ok().flatten())
        .filter_map(|(value, _)| value.into_string().ok())
        .collect()
}

async fn wait(hub: &Hub, id: &DocId, len: usize) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if list(hub, id).len() >= len {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err("peers did not find each other within 20 s; is multicast blocked on this network?".into())
}
