//! CD-3: peers sync through a relay on localhost.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use lampstand_relay::{Config, Relay};
use lampstand_sync::automerge::transaction::Transactable;
use lampstand_sync::automerge::{ObjType, ReadDoc, ROOT};
use lampstand_sync::{client, ChurchCredentials, DocId, Hub};
use tokio::task::JoinHandle;

fn service() -> DocId {
    DocId::new("services/2026-10-18-am").unwrap()
}

fn plan(hub: &Hub, first_item: &str) {
    hub.create(&service(), |doc| {
        doc.put_object(ROOT, "items", ObjType::List).map(drop)
    })
    .unwrap();
    add_item(hub, first_item);
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

/// Starts a relay on a free localhost port; returns its ws:// URL.
async fn start_relay(data: &Path, churches: Vec<ChurchCredentials>) -> (String, JoinHandle<()>) {
    let relay = Relay::bind(Config {
        listen: "127.0.0.1:0".parse().unwrap(),
        data_dir: data.to_path_buf(),
        churches,
    })
    .await
    .unwrap();
    let url = format!("ws://{}", relay.local_addr().unwrap());
    (url, tokio::spawn(relay.run()))
}

fn hub() -> (Arc<Hub>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    (Hub::open(dir.path()).unwrap(), dir)
}

#[tokio::test]
async fn two_peers_converge_through_relay() {
    let relay_dir = tempfile::tempdir().unwrap();
    let grace = ChurchCredentials::generate("grace").unwrap();
    let (url, _relay) = start_relay(relay_dir.path(), vec![grace.clone()]).await;
    let ((home, _d1), (church, _d2)) = (hub(), hub());

    plan(&home, "Welcome");
    let _h = client::spawn_relay_sync(home.clone(), url.clone(), grace.clone());
    let _c = client::spawn_relay_sync(church.clone(), url, grace);

    eventually("church laptop to get the plan", || {
        items(&church) == ["Welcome"]
    })
    .await;
    add_item(&church, "Amazing Grace");
    add_item(&home, "Notices");
    eventually("both sides to agree", || {
        items(&home).len() == 3 && items(&home) == items(&church)
    })
    .await;
    assert_eq!(
        home.store().heads(&service()),
        church.store().heads(&service())
    );
}

#[tokio::test]
async fn relay_stores_and_forwards_across_restarts() {
    let relay_dir = tempfile::tempdir().unwrap();
    let grace = ChurchCredentials::generate("grace").unwrap();
    let (url, relay) = start_relay(relay_dir.path(), vec![grace.clone()]).await;

    // Tuesday: the planner syncs from home, then goes offline.
    let (home, _d1) = hub();
    plan(&home, "Welcome");
    add_item(&home, "Be Thou My Vision");
    let planner = client::spawn_relay_sync(home.clone(), url.clone(), grace.clone());
    // A second device confirms the relay holds both changes.
    let (check, _d3) = hub();
    let checker = client::spawn_relay_sync(check.clone(), url.clone(), grace.clone());
    eventually("relay to hold both items", || items(&check).len() == 2).await;
    checker.abort();
    planner.abort();

    // The relay restarts (an update, a reboot of the Pi).
    relay.abort();
    let _ = relay.await;
    let (url, _relay) = start_relay(relay_dir.path(), vec![grace.clone()]).await;

    // Sunday: the projection laptop connects for the first time.
    let (laptop, _d2) = hub();
    let _l = client::spawn_relay_sync(laptop.clone(), url, grace);
    eventually("laptop to get Tuesday's plan", || {
        items(&laptop) == ["Welcome", "Be Thou My Vision"]
    })
    .await;
}

#[tokio::test]
async fn relay_keeps_churches_apart_and_strangers_out() {
    let relay_dir = tempfile::tempdir().unwrap();
    let grace = ChurchCredentials::generate("grace").unwrap();
    let hope = ChurchCredentials::generate("hope").unwrap();
    let (url, _relay) = start_relay(relay_dir.path(), vec![grace.clone(), hope.clone()]).await;

    let (grace_hub, _d1) = hub();
    plan(&grace_hub, "Welcome");
    let _g = client::spawn_relay_sync(grace_hub, url.clone(), grace.clone());

    // Wrong token, unknown church, and a real church's token used for
    // another church are all refused.
    let stranger = ChurchCredentials::new("grace", "0".repeat(64)).unwrap();
    assert!(client::connect(&url, &stranger).await.is_err());
    let unknown = ChurchCredentials::generate("mallory").unwrap();
    assert!(client::connect(&url, &unknown).await.is_err());
    let swapped = ChurchCredentials::new("grace", hope.token()).unwrap();
    assert!(client::connect(&url, &swapped).await.is_err());

    // Hope syncs fine but never sees Grace's service.
    let (hope_hub, _d2) = hub();
    plan(&hope_hub, "Hope's welcome");
    assert!(client::connect(&url, &hope).await.is_ok());
    let _h = client::spawn_relay_sync(hope_hub.clone(), url, hope);
    eventually("relay to store Grace's plan", || {
        relay_dir
            .path()
            .join("grace/docs/services/2026-10-18-am.automerge")
            .exists()
    })
    .await;
    eventually("relay to store Hope's plan", || {
        relay_dir
            .path()
            .join("hope/docs/services/2026-10-18-am.automerge")
            .exists()
    })
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(items(&hope_hub), ["Hope's welcome"]);
}
