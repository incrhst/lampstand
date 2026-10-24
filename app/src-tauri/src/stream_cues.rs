//! Who owns the OBS connection: the Stream holder (decision 0010, point 6).
//!
//! `obs_control` is the same on every device: one OBS connection, a follower
//! that applies the live item's scene, the drawer's requests. What this
//! module decides is which device's copy is in use.
//!
//! - **The Stream holder owns OBS.** A cue made when an item goes live is
//!   sent over sync to the device that holds Stream, which hands it to its
//!   own `ObsControl`. The Projection holder does not touch its own OBS
//!   while Stream is held by another device, so two devices never both
//!   switch.
//! - **Nobody holds Stream:** the Projection holder keeps the duty, as in
//!   0.7.0. A device with nobody paired holds all four roles, so it drives
//!   its own OBS and nothing about it changes.
//! - **The drawer follows.** A device that is not the owner reports OBS as
//!   not relevant, so Present shows no OBS strip, warning or drawer there,
//!   and it refuses to switch scenes or mutes by hand. Setup › OBS still
//!   works everywhere, so a machine can be set up before it takes Stream.
//!
//! The OBS password and any file path never leave the device: a cue is a
//! scene name or nothing, and Stream holders answer with an acknowledgement
//! that carries no data.

use std::sync::Arc;
use std::time::Duration;

use lampstand_api::{Event, ObsRequest, ObsView, Response};
use lampstand_core::service::ItemId;
use lampstand_sync::net::{CueRoute, SyncNode};
use tokio::sync::mpsc;

use crate::handler::EventSink;
use crate::obs_control::{SceneCue, SceneCues};
use crate::sync::NodeCell;

/// What a hand pick of a scene or a mute says on a device that does not own
/// OBS. The drawer is not shown there, so this is a safety net, not a screen.
pub const NOT_THE_STREAM: &str = "OBS is run by the device that runs the stream.";

/// Whether this device owns the OBS connection now. True until sync exists.
pub fn obs_here(cell: &NodeCell) -> bool {
    cell.get().is_none_or(|node| node.obs_here())
}

/// The view as this device should show it: OBS is not worth mentioning on a
/// device that does not own it.
pub fn shape(mut view: ObsView, here: bool) -> ObsView {
    view.relevant &= here;
    view
}

/// `emit`, with `Event::Obs` shaped for this device.
pub fn shaping(emit: EventSink, cell: NodeCell) -> EventSink {
    Arc::new(move |event: Event| match event {
        Event::Obs(view) => emit(Event::Obs(Box::new(shape(*view, obs_here(&cell))))),
        event => emit(event),
    })
}

/// The answer to an `Obs` request, shaped for this device.
pub fn shape_response(response: Response, here: bool) -> Response {
    match response {
        Response::Obs(view) => Response::Obs(shape(view, here)),
        other => other,
    }
}

/// Whether `request` switches OBS from here and so must not run on a device
/// that does not own it.
pub fn switches_obs(request: &ObsRequest) -> bool {
    matches!(
        request,
        ObsRequest::SetScene { .. } | ObsRequest::Resume | ObsRequest::SetMute { .. }
    )
}

/// On the Stream holder: cues from the Projection holder go to this device's
/// own OBS through `local`.
pub fn accept(node: &SyncNode, local: SceneCues) {
    node.set_scene_cue_hook(Arc::new(move |scene| {
        let _ = local.send(SceneCue {
            item: ItemId::generate(),
            scene,
        });
    }));
}

/// Reads the current `ObsView`, for telling the screen when ownership moves.
pub type ViewFn = Arc<dyn Fn() -> ObsView + Send + Sync>;

/// Where `App` sends the scene of each item that goes live. A task decides,
/// for each cue, whether this device's OBS applies it (`local`) or the
/// Stream holder's does (over sync), and tells the screen when this device
/// gains or loses the OBS duty. Call inside the Tokio runtime.
pub fn start_router(cell: NodeCell, local: SceneCues, view: ViewFn, emit: EventSink) -> SceneCues {
    let (tx, mut rx) = mpsc::unbounded_channel::<SceneCue>();
    tokio::spawn(async move {
        // Until sync exists (or when it is off) this device is alone.
        let node = loop {
            if let Some(node) = cell.get() {
                break node.clone();
            }
            tokio::select! {
                cue = rx.recv() => match cue {
                    Some(cue) => { let _ = local.send(cue); }
                    None => return,
                },
                _ = tokio::time::sleep(Duration::from_millis(250)) => {}
            }
        };
        let mut changes = node.watch();
        let mut here = node.obs_here();
        loop {
            tokio::select! {
                cue = rx.recv() => {
                    let Some(cue) = cue else { return };
                    match node.route_scene_cue(cue.scene.clone()) {
                        CueRoute::Local => { let _ = local.send(cue); }
                        // Forget any switch this device's OBS was waiting
                        // to make: it is not this device's to make now.
                        CueRoute::Sent => {
                            let _ = local.send(SceneCue { item: cue.item, scene: None });
                        }
                        CueRoute::Dropped => {}
                    }
                }
                changed = changes.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    let now = node.obs_here();
                    if now != here {
                        here = now;
                        if !now {
                            let _ = local.send(SceneCue { item: ItemId::generate(), scene: None });
                        }
                        emit(Event::Obs(Box::new(shape(view(), now))));
                    }
                }
            }
        }
    });
    tx
}
