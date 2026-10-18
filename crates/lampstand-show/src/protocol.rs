//! The WebSocket protocol between the server and its clients: the operator
//! page, the output process, the stage display and the OBS stream page.
//!
//! Every message is one JSON text frame tagged by `type`. See `docs/v0.1.md`.

use serde::{Deserialize, Serialize};

/// The default port for the local server.
pub const DEFAULT_PORT: u16 = 7420;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Operator,
    Output,
    Stage,
    Stream,
}

/// Client → server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Hello { role: Role, name: String },
    Command { command: Command },
}

/// Server → client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    State(Box<Snapshot>),
    /// A command was refused, e.g. from a client that may not send commands.
    Error { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// The live cue is shown.
    #[default]
    Normal,
    /// Text removed, background stays.
    Clear,
    Black,
    Logo,
}

/// A position in the service: item index, cue index within the item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Pos {
    pub item: usize,
    pub cue: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Command {
    /// Preview goes live; preview moves to the cue after it.
    GoLive,
    /// Live moves one cue forward through the service (preview goes live if nothing is).
    Next,
    Prev,
    /// Move preview.
    Select { pos: Pos },
    /// Move preview and live.
    GoTo { pos: Pos },
    /// Switch mode; asking for the current mode again returns to `Normal`.
    Mode { mode: Mode },
    /// Nothing live.
    ClearLive,
}

/// What a client draws for one cue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CueView {
    Text {
        lines: Vec<String>,
        /// Copyright line for songs, reference for scripture.
        footer: Option<String>,
        /// Short label for the operator and stage, e.g. "V1", "Jn 3:16-17".
        label: String,
        notes: Option<String>,
    },
    Image {
        /// Server-relative; append `?w=PIXELS`.
        url: String,
        /// Width over height.
        aspect: f32,
        label: String,
        notes: Option<String>,
    },
}

impl CueView {
    pub fn label(&self) -> &str {
        match self {
            CueView::Text { label, .. } | CueView::Image { label, .. } => label,
        }
    }

    pub fn notes(&self) -> Option<&str> {
        match self {
            CueView::Text { notes, .. } | CueView::Image { notes, .. } => notes.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Song,
    Scripture,
    Deck,
    Logo,
}

/// One service item as clients see it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemView {
    pub id: String,
    pub kind: ItemKind,
    pub title: String,
    /// One label per cue, e.g. ["V1", "C", "V2", "C"].
    pub cues: Vec<String>,
    /// Set when the item could not be expanded (missing song, bad reference, unreadable deck).
    pub problem: Option<String>,
}

/// The whole state a client needs, sent after every change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub service_name: String,
    pub items: Vec<ItemView>,
    pub preview: Option<Pos>,
    pub live: Option<Pos>,
    pub mode: Mode,
    pub preview_cue: Option<CueView>,
    pub live_cue: Option<CueView>,
    /// The cue `next` would take live.
    pub next_cue: Option<CueView>,
    /// Server-relative logo URL, if a logo is set.
    pub logo: Option<String>,
    /// Unix milliseconds: when the first cue went live, and when the live item did.
    pub service_started_at: Option<u64>,
    pub item_started_at: Option<u64>,
    /// Increases with every change, so clients can drop stale frames.
    pub revision: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_format_matches_the_doc() {
        let m: ClientMessage = serde_json::from_str(r#"{"type":"command","command":{"kind":"next"}}"#).unwrap();
        assert_eq!(m, ClientMessage::Command { command: Command::Next });
        let m: ClientMessage =
            serde_json::from_str(r#"{"type":"command","command":{"kind":"mode","mode":"black"}}"#).unwrap();
        assert_eq!(m, ClientMessage::Command { command: Command::Mode { mode: Mode::Black } });
        let m: ClientMessage = serde_json::from_str(r#"{"type":"hello","role":"stage","name":"Stage left"}"#).unwrap();
        assert_eq!(m, ClientMessage::Hello { role: Role::Stage, name: "Stage left".into() });
        let v = CueView::Image { url: "/media/deck/a/0/0.png".into(), aspect: 1.5, label: "Slide 1".into(), notes: None };
        assert_eq!(
            serde_json::to_string(&v).unwrap(),
            r#"{"kind":"image","url":"/media/deck/a/0/0.png","aspect":1.5,"label":"Slide 1","notes":null}"#
        );
    }
}
