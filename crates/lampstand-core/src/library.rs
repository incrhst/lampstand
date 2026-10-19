//! The church library: every song and service, held in one Automerge
//! document so each device keeps a full copy and edits merge.
//!
//! PRD: CD-1.

use std::collections::BTreeMap;

use autosurgeon::{Hydrate, Reconcile};
use serde::{Deserialize, Serialize};

use crate::service::Service;
use crate::song::Song;

/// The root of the Automerge document. Songs and services are maps keyed by
/// id, so devices adding different songs never conflict.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, Reconcile, Hydrate)]
pub struct Library {
    pub songs: BTreeMap<String, Song>,
    pub services: BTreeMap<String, Service>,
}
