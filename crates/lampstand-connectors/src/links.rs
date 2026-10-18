//! Pasted share links (PS-1 OneDrive links, CD-9 guest links): no account
//! needed when the file is shared as "anyone with the link".
//!
//! - Google Slides: `https://docs.google.com/presentation/d/<id>/...`
//!   downloads from `.../d/<id>/export/pptx`.
//! - Google Drive file: `https://drive.google.com/file/d/<id>/...` or
//!   `open?id=<id>` downloads from
//!   `https://drive.google.com/uc?export=download&id=<id>`.
//! - OneDrive and SharePoint: `1drv.ms/...`, `onedrive.live.com/...`,
//!   `<tenant>.sharepoint.com/...` download through the shares API,
//!   `https://api.onedrive.com/v1.0/shares/u!<base64url(url)>/root/content`.

use crate::{FetchError, Fetched};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    GoogleSlides { id: String },
    GoogleDriveFile { id: String },
    OneDrive { url: String },
}

/// Recognises a pasted link, tolerating surrounding spaces and missing
/// `https://`. `None` for anything else.
pub fn classify(input: &str) -> Option<Link> {
    let _ = input;
    todo!("T39")
}

/// Downloads the file behind a link. Google Slides come back as PPTX.
pub async fn fetch(
    link: &Link,
    previous_version: Option<&str>,
) -> Result<Option<Fetched>, FetchError> {
    let _ = (link, previous_version);
    todo!("T39")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "T39"]
    fn classifies_links() {
        let id = "1AbC-dEf_123";
        assert_eq!(
            classify(&format!(
                "https://docs.google.com/presentation/d/{id}/edit?usp=sharing"
            )),
            Some(Link::GoogleSlides { id: id.into() })
        );
        assert_eq!(
            classify(&format!(" docs.google.com/presentation/d/{id}/pub ")),
            Some(Link::GoogleSlides { id: id.into() })
        );
        assert_eq!(
            classify(&format!("https://drive.google.com/file/d/{id}/view")),
            Some(Link::GoogleDriveFile { id: id.into() })
        );
        assert_eq!(
            classify(&format!("https://drive.google.com/open?id={id}")),
            Some(Link::GoogleDriveFile { id: id.into() })
        );
        for url in [
            "https://1drv.ms/p/s!AbCdEf",
            "https://contoso.sharepoint.com/:p:/s/team/EabC",
        ] {
            assert_eq!(classify(url), Some(Link::OneDrive { url: url.into() }));
        }
        assert_eq!(classify("https://example.com/deck.pptx"), None);
        assert_eq!(classify("not a link"), None);
    }
}
