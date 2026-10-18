//! CD-2: two peers on the LAN sync edits both ways, and a peer with the wrong
//! church token gets nothing. Peers are wired by address here; mDNS
//! discovery is shown by `cargo run --example lan_two_peers`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use lampstand_sync::automerge::transaction::Transactable;
use lampstand_sync::automerge::{ObjType, ReadDoc, ROOT};
use lampstand_sync::{client, ChurchCredentials, DocId, Hub, LanPeer};

fn service() -> DocId {
    DocId::new("services/2026-10-18-am").unwrap()
}

fn add_item(hub: &Hub, item: &str) {
    hub.edit(&service(), &format!("add {item}"), |doc| {
        let (_, items) = doc.get(ROOT, "items")?.expect("items list");
        let len = doc.length(&items);
        doc.insert(&items, len, item)?;
        Ok(())
    })
    .unwrap();
}

fn items(hub: &Hub) -> Vec<String> {
    let mut store = hub.store();
    let Some(doc) = store.doc(&service()) else {
        return vec![];
    };
    let Some((_, items)) = doc.get(ROOT, "items").unwrap() else {
        return vec![];
    };
    (0..doc.length(&items))
        .map(|i| {
            doc.get(&items, i)
                .unwrap()
                .unwrap()
                .0
                .into_string()
                .unwrap()
        })
        .collect()
}

async fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    for _ in 0..500 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for {what}");
}

fn localhost() -> SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}

#[tokio::test]
async fn two_lan_peers_converge() {
    let creds = ChurchCredentials::generate("grace").unwrap();
    let (dir_a, dir_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let a = Hub::open(dir_a.path()).unwrap();
    let b = Hub::open(dir_b.path()).unwrap();

    // Office PC plans the service before the laptop is on the network.
    a.create(&service(), |doc| {
        doc.put_object(ROOT, "items", ObjType::List).map(drop)
    })
    .unwrap();
    add_item(&a, "Welcome");

    let _peer_a = LanPeer::start(a.clone(), creds.clone(), localhost(), false)
        .await
        .unwrap();
    let mut peer_b = LanPeer::start(b.clone(), creds.clone(), localhost(), false)
        .await
        .unwrap();
    peer_b.connect(format!("ws://{}", _peer_a.local_addr()));

    eventually("laptop to receive the plan", || items(&b) == ["Welcome"]).await;
    add_item(&b, "Amazing Grace");
    eventually("office to see the laptop's edit", || items(&a).len() == 2).await;
    add_item(&a, "Notices");
    eventually("both to agree", || {
        items(&a) == items(&b) && items(&b).len() == 3
    })
    .await;
    assert_eq!(a.store().heads(&service()), b.store().heads(&service()));
}

#[tokio::test]
async fn wrong_token_is_refused() {
    let dir_a = tempfile::tempdir().unwrap();
    let a = Hub::open(dir_a.path()).unwrap();
    a.create(&service(), |doc| {
        doc.put_object(ROOT, "items", ObjType::List).map(drop)
    })
    .unwrap();
    let creds = ChurchCredentials::generate("grace").unwrap();
    let peer_a = LanPeer::start(a, creds, localhost(), false).await.unwrap();
    let base = format!("ws://{}", peer_a.local_addr());

    let stranger = ChurchCredentials::generate("grace").unwrap();
    assert!(client::connect(&base, &stranger).await.is_err());
    let other_church = ChurchCredentials::new("hope", "x").unwrap();
    assert!(client::connect(&base, &other_church).await.is_err());

    let dir_b = tempfile::tempdir().unwrap();
    let b: Arc<Hub> = Hub::open(dir_b.path()).unwrap();
    assert!(client::sync_with(b.clone(), &base, &stranger)
        .await
        .is_err());
    assert!(items(&b).is_empty());
}
