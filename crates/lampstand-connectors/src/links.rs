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

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use reqwest::header::{
    HeaderMap, CONTENT_DISPOSITION, CONTENT_TYPE, ETAG, IF_NONE_MATCH, LAST_MODIFIED,
};
use reqwest::StatusCode;
use url::Url;

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
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    let url = if input.contains("://") {
        Url::parse(input).ok()?
    } else {
        Url::parse(&format!("https://{input}")).ok()?
    };
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    let segments: Vec<&str> = url.path_segments()?.collect();

    match host.as_str() {
        "docs.google.com" => match segments.as_slice() {
            ["presentation", "d", id, ..] if valid_id(id) => {
                Some(Link::GoogleSlides { id: (*id).into() })
            }
            _ => None,
        },
        "drive.google.com" => match segments.as_slice() {
            ["file", "d", id, ..] if valid_id(id) => {
                Some(Link::GoogleDriveFile { id: (*id).into() })
            }
            ["open"] | ["uc"] => url
                .query_pairs()
                .find(|(k, _)| k == "id")
                .filter(|(_, v)| valid_id(v))
                .map(|(_, v)| Link::GoogleDriveFile { id: v.into_owned() }),
            _ => None,
        },
        h if h == "1drv.ms"
            || h == "onedrive.live.com"
            || h == "sharepoint.com"
            || h.ends_with(".sharepoint.com") =>
        {
            Some(Link::OneDrive {
                url: if input.contains("://") {
                    input.to_string()
                } else {
                    url.to_string()
                },
            })
        }
        _ => None,
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The direct-download URL for a link.
fn download_url(link: &Link) -> String {
    match link {
        Link::GoogleSlides { id } => {
            format!("https://docs.google.com/presentation/d/{id}/export/pptx")
        }
        Link::GoogleDriveFile { id } => {
            format!("https://drive.google.com/uc?export=download&id={id}")
        }
        Link::OneDrive { url } => format!(
            "https://api.onedrive.com/v1.0/shares/u!{}/root/content",
            URL_SAFE_NO_PAD.encode(url)
        ),
    }
}

fn default_name(link: &Link) -> String {
    match link {
        Link::GoogleSlides { id } | Link::GoogleDriveFile { id } => format!("{id}.pptx"),
        Link::OneDrive { .. } => "onedrive.pptx".into(),
    }
}

/// Downloads the file behind a link. Google Slides come back as PPTX.
///
/// `previous_version` is the version marker of an earlier fetch; when the
/// source reports the same version, the result is `Ok(None)`.
pub async fn fetch(
    link: &Link,
    previous_version: Option<&str>,
) -> Result<Option<Fetched>, FetchError> {
    fetch_url(&download_url(link), &default_name(link), previous_version).await
}

async fn fetch_url(
    url: &str,
    fallback_name: &str,
    previous_version: Option<&str>,
) -> Result<Option<Fetched>, FetchError> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("lampstand/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| FetchError::Network(e.to_string()))?;
    let mut request = client.get(url);
    if let Some(v) = previous_version {
        request = request.header(IF_NONE_MATCH, v);
    }
    let response = request
        .send()
        .await
        .map_err(|e| FetchError::Network(e.to_string()))?;

    let status = response.status();
    match status {
        StatusCode::NOT_MODIFIED => return Ok(None),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => return Err(FetchError::Forbidden),
        StatusCode::NOT_FOUND | StatusCode::GONE => return Err(FetchError::NotFound),
        s if !s.is_success() => return Err(FetchError::Network(format!("HTTP {s}"))),
        _ => {}
    }

    let headers = response.headers().clone();
    let version = version_of(&headers);
    if version.is_some() && version.as_deref() == previous_version {
        return Ok(None);
    }
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let name = headers
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(filename_from_disposition)
        .unwrap_or_else(|| fallback_name.to_string());

    let mime = content_type.split(';').next().unwrap_or("").trim();
    if mime.eq_ignore_ascii_case("text/html") || mime.eq_ignore_ascii_case("application/xhtml+xml")
    {
        return Err(FetchError::NotADeck(content_type));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| FetchError::Network(e.to_string()))?
        .to_vec();
    // PPTX and PPT-in-OOXML are zip containers.
    if !bytes.starts_with(b"PK") {
        return Err(FetchError::NotADeck(content_type));
    }
    Ok(Some(Fetched {
        name,
        content_type,
        bytes,
        version,
    }))
}

fn version_of(headers: &HeaderMap) -> Option<String> {
    [ETAG, LAST_MODIFIED]
        .iter()
        .find_map(|h| headers.get(h))
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

fn filename_from_disposition(value: &str) -> Option<String> {
    let param = |key: &str| {
        value.split(';').skip(1).find_map(|part| {
            let (k, v) = part.split_once('=')?;
            (k.trim().eq_ignore_ascii_case(key)).then(|| v.trim().trim_matches('"').to_string())
        })
    };
    if let Some(v) = param("filename*") {
        // RFC 5987: charset'lang'percent-encoded
        let encoded = v.rsplit('\'').next().unwrap_or(&v);
        let decoded = percent_decode(encoded);
        if !decoded.is_empty() {
            return Some(decoded);
        }
    }
    param("filename").filter(|n| !n.is_empty())
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
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

    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const PPTX: &[u8] = b"PK\x03\x04fake-pptx";

    fn deck_response() -> ResponseTemplate {
        ResponseTemplate::new(200)
            .insert_header(
                "content-type",
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            )
            .insert_header(
                "content-disposition",
                "attachment; filename=\"Sunday.pptx\"",
            )
            .insert_header("etag", "\"v1\"")
            .set_body_bytes(PPTX)
    }

    async fn serve(template: ResponseTemplate) -> (MockServer, String) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/deck"))
            .respond_with(template)
            .mount(&server)
            .await;
        let url = format!("{}/deck", server.uri());
        (server, url)
    }

    #[test]
    fn builds_download_urls() {
        assert_eq!(
            download_url(&Link::GoogleSlides { id: "abc".into() }),
            "https://docs.google.com/presentation/d/abc/export/pptx"
        );
        assert_eq!(
            download_url(&Link::GoogleDriveFile { id: "abc".into() }),
            "https://drive.google.com/uc?export=download&id=abc"
        );
        // base64url of "https://1drv.ms/p/s!A" without padding
        assert_eq!(
            download_url(&Link::OneDrive {
                url: "https://1drv.ms/p/s!A".into()
            }),
            "https://api.onedrive.com/v1.0/shares/u!aHR0cHM6Ly8xZHJ2Lm1zL3AvcyFB/root/content"
        );
    }

    #[test]
    fn parses_content_disposition() {
        assert_eq!(
            filename_from_disposition("attachment; filename=\"a b.pptx\"").as_deref(),
            Some("a b.pptx")
        );
        assert_eq!(
            filename_from_disposition(
                "attachment; filename=\"x.pptx\"; filename*=UTF-8''Caf%C3%A9.pptx"
            )
            .as_deref(),
            Some("Café.pptx")
        );
        assert_eq!(filename_from_disposition("inline"), None);
    }

    #[tokio::test]
    async fn fetch_success() {
        let (_server, url) = serve(deck_response()).await;
        let got = fetch_url(&url, "fallback.pptx", None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.name, "Sunday.pptx");
        assert_eq!(got.bytes, PPTX);
        assert_eq!(got.version.as_deref(), Some("\"v1\""));
    }

    #[tokio::test]
    async fn fetch_unchanged_via_304() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(header("if-none-match", "\"v1\""))
            .respond_with(ResponseTemplate::new(304))
            .mount(&server)
            .await;
        let url = format!("{}/deck", server.uri());
        assert_eq!(
            fetch_url(&url, "f.pptx", Some("\"v1\"")).await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn fetch_unchanged_when_server_ignores_conditional() {
        let (_server, url) = serve(deck_response()).await;
        assert_eq!(
            fetch_url(&url, "f.pptx", Some("\"v1\"")).await.unwrap(),
            None
        );
        // A different previous version downloads again.
        assert!(fetch_url(&url, "f.pptx", Some("\"v0\""))
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn fetch_uses_fallback_name() {
        let (_server, url) = serve(ResponseTemplate::new(200).set_body_bytes(PPTX)).await;
        let got = fetch_url(&url, "fallback.pptx", None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.name, "fallback.pptx");
        assert_eq!(got.version, None);
    }

    #[tokio::test]
    async fn fetch_forbidden() {
        let (_server, url) = serve(ResponseTemplate::new(403)).await;
        assert!(matches!(
            fetch_url(&url, "f.pptx", None).await,
            Err(FetchError::Forbidden)
        ));
    }

    #[tokio::test]
    async fn fetch_not_found() {
        let (_server, url) = serve(ResponseTemplate::new(404)).await;
        assert!(matches!(
            fetch_url(&url, "f.pptx", None).await,
            Err(FetchError::NotFound)
        ));
    }

    #[tokio::test]
    async fn fetch_server_error_is_network() {
        let (_server, url) = serve(ResponseTemplate::new(500)).await;
        assert!(matches!(
            fetch_url(&url, "f.pptx", None).await,
            Err(FetchError::Network(_))
        ));
    }

    #[tokio::test]
    async fn fetch_html_is_not_a_deck() {
        let (_server, url) = serve(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/html; charset=utf-8")
                .set_body_string("<html>Sign in</html>"),
        )
        .await;
        assert!(matches!(
            fetch_url(&url, "f.pptx", None).await,
            Err(FetchError::NotADeck(_))
        ));
    }

    #[tokio::test]
    async fn fetch_non_zip_body_is_not_a_deck() {
        let (_server, url) = serve(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/octet-stream")
                .set_body_string("plain text"),
        )
        .await;
        assert!(matches!(
            fetch_url(&url, "f.pptx", None).await,
            Err(FetchError::NotADeck(_))
        ));
    }

    /// Hits the real Google Slides export endpoint. Set `LAMPSTAND_LIVE_SLIDES_ID`
    /// to the ID of a deck shared as "anyone with the link", then run
    /// `cargo test -p lampstand-connectors -- --ignored live_google_slides`.
    #[tokio::test]
    #[ignore = "needs network and a public deck ID in LAMPSTAND_LIVE_SLIDES_ID"]
    async fn live_google_slides() {
        let id = std::env::var("LAMPSTAND_LIVE_SLIDES_ID")
            .expect("set LAMPSTAND_LIVE_SLIDES_ID to a public Google Slides deck ID");
        let link = classify(&format!("https://docs.google.com/presentation/d/{id}/edit"))
            .expect("classifies");
        let got = fetch(&link, None).await.unwrap().unwrap();
        assert!(got.bytes.starts_with(b"PK"));
    }
}
