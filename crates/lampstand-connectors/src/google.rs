//! Google account connection (PS-2): OAuth with the `drive.file` scope
//! only, so no restricted-scope security review.
//!
//! - Sign-in: OAuth 2.0 for installed apps with PKCE and a loopback
//!   redirect (RFC 8252) in the system browser. Never in a webview: Google
//!   blocks embedded sign-in. Tokens go in the OS keychain.
//! - Picking: the Google Picker needs a web origin Google accepts, which
//!   the Tauri webview's custom scheme is not. The local server serves
//!   `http://localhost:<port>/google/picker`, opened in the system
//!   browser; it posts the picked file IDs back to the local server.
//!   Picking grants `drive.file` access to those files.
//! - Fetching: Google Slides files through `files.export` to PPTX; every
//!   other file through `files.get?alt=media`. `version` from
//!   `files.get?fields=version` is the update marker (PS-5).

use crate::{FetchError, Fetched};

pub struct GoogleAccount {
    _private: (),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickedFile {
    pub id: String,
    pub name: String,
    pub mime_type: String,
}

impl GoogleAccount {
    pub async fn sign_in() -> Result<Self, FetchError> {
        todo!("T40")
    }

    /// Opens the picker in the system browser and waits for the choice.
    pub async fn pick(&self, picker_url: &str) -> Result<Vec<PickedFile>, FetchError> {
        let _ = picker_url;
        todo!("T40")
    }

    /// `None` if the file's version still equals `previous_version`.
    pub async fn fetch(
        &self,
        file_id: &str,
        previous_version: Option<&str>,
    ) -> Result<Option<Fetched>, FetchError> {
        let _ = (file_id, previous_version);
        todo!("T40")
    }
}
