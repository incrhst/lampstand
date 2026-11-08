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
//!
//! Sound (T113) takes Drive, OneDrive, Dropbox (`dl=1`) and direct `https://`
//! links to sound files, and refuses streaming sites before any request:
//! see [`classify_sound`] and [`fetch_sound`].

use std::io::Write;
use std::path::{Path, PathBuf};

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

/// The most a sound link downloads: 250 MB.
pub const MAX_SOUND_BYTES: u64 = 250 * 1024 * 1024;

/// Extensions of the sound files a plain link may name.
const SOUND_EXTENSIONS: [&str; 7] = ["mp3", "wav", "m4a", "aac", "flac", "ogg", "opus"];

/// Streaming sites. Their pages are not files and their terms forbid
/// taking the audio out of them, so a link to one is refused before any
/// request is made.
const STREAMING_HOSTS: [&str; 8] = [
    "youtube.com",
    "youtu.be",
    "youtube-nocookie.com",
    "spotify.com",
    "spotify.link",
    "music.apple.com",
    "itunes.apple.com",
    "soundcloud.com",
];

/// A pasted link to a sound file, with the address its bytes come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundLink {
    /// The direct-download address.
    pub download: String,
    /// The file's name when the response does not give one: the last part
    /// of the link, if it has one.
    pub fallback_name: String,
}

/// Why a pasted link is not taken as a sound file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundLinkError {
    /// YouTube, Spotify, Apple Music or SoundCloud.
    Streaming,
    /// Anything else that is not a Drive, Dropbox, OneDrive or direct link.
    Unsupported,
}

#[derive(Debug)]
pub enum SoundFetchError {
    Network(String),
    /// The link is private or the account has no access.
    Forbidden,
    NotFound,
    /// The link leads to a web page, not a file.
    WebPage,
    /// More than the limit.
    TooLarge,
    /// The temporary file could not be written.
    Disk(String),
}

fn is_streaming(host: &str) -> bool {
    STREAMING_HOSTS
        .iter()
        .any(|h| host == *h || host.strip_suffix(h).is_some_and(|p| p.ends_with('.')))
}

fn last_part(url: &Url) -> String {
    url.path_segments()
        .and_then(|mut s| s.rfind(|p| !p.is_empty()))
        .map(percent_decode)
        .unwrap_or_default()
}

fn has_sound_extension(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| SOUND_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Recognises a pasted link to a sound file: a Google Drive file, OneDrive,
/// Dropbox (fetched with `dl=1`) or an `https://` link to a file whose name
/// ends in a sound extension. Streaming sites are refused first, so nothing
/// is requested for them.
pub fn classify_sound(input: &str) -> Result<SoundLink, SoundLinkError> {
    let input = input.trim();
    let url = if input.contains("://") {
        Url::parse(input)
    } else {
        Url::parse(&format!("https://{input}"))
    }
    .map_err(|_| SoundLinkError::Unsupported)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(SoundLinkError::Unsupported);
    }
    let host = url
        .host_str()
        .ok_or(SoundLinkError::Unsupported)?
        .to_ascii_lowercase();
    if is_streaming(&host) {
        return Err(SoundLinkError::Streaming);
    }

    if host == "dropbox.com" || host.ends_with(".dropbox.com") {
        // A shared folder is not a file.
        let segments: Vec<&str> = url.path_segments().map(|s| s.collect()).unwrap_or_default();
        let folder = matches!(segments.as_slice(), ["sh", ..] | ["scl", "fo", ..]);
        if folder || url.scheme() != "https" {
            return Err(SoundLinkError::Unsupported);
        }
        let mut direct = url.clone();
        let kept: Vec<(String, String)> = url
            .query_pairs()
            .filter(|(k, _)| k != "dl")
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        direct
            .query_pairs_mut()
            .clear()
            .extend_pairs(kept)
            .append_pair("dl", "1");
        return Ok(SoundLink {
            download: direct.to_string(),
            fallback_name: last_part(&url),
        });
    }

    match classify(input) {
        Some(link @ (Link::GoogleDriveFile { .. } | Link::OneDrive { .. })) => Ok(SoundLink {
            download: download_url(&link),
            fallback_name: String::new(),
        }),
        // A Google Slides link is a presentation.
        Some(Link::GoogleSlides { .. }) => Err(SoundLinkError::Unsupported),
        None => {
            let name = last_part(&url);
            if url.scheme() == "https" && has_sound_extension(&name) {
                Ok(SoundLink {
                    download: url.to_string(),
                    fallback_name: name,
                })
            } else {
                Err(SoundLinkError::Unsupported)
            }
        }
    }
}

/// Downloads the sound behind a link into `dir` (which must exist), under
/// the file's own name, and returns its path. The bytes are streamed to
/// disk, never held in memory, and the download stops at 250 MB. The caller
/// checks the file is sound and removes `dir`.
pub async fn fetch_sound(link: &SoundLink, dir: &Path) -> Result<PathBuf, SoundFetchError> {
    fetch_sound_limited(link, dir, MAX_SOUND_BYTES).await
}

async fn fetch_sound_limited(
    link: &SoundLink,
    dir: &Path,
    max_bytes: u64,
) -> Result<PathBuf, SoundFetchError> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("lampstand/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| SoundFetchError::Network(e.to_string()))?;
    let mut response = client
        .get(&link.download)
        .send()
        .await
        .map_err(|e| SoundFetchError::Network(e.to_string()))?;
    match response.status() {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => return Err(SoundFetchError::Forbidden),
        StatusCode::NOT_FOUND | StatusCode::GONE => return Err(SoundFetchError::NotFound),
        s if !s.is_success() => return Err(SoundFetchError::Network(format!("HTTP {s}"))),
        _ => {}
    }
    let mime = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if mime == "text/html" || mime == "application/xhtml+xml" {
        return Err(SoundFetchError::WebPage);
    }
    if response.content_length().is_some_and(|n| n > max_bytes) {
        return Err(SoundFetchError::TooLarge);
    }

    let name = response
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(filename_from_disposition)
        .unwrap_or_else(|| link.fallback_name.clone());
    // Only the last part of a name: it must not point outside `dir`.
    let name = Path::new(&name.replace('\\', "/"))
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty() && *n != "..")
        .unwrap_or("Track")
        .to_string();

    let io = |e: std::io::Error| SoundFetchError::Disk(e.to_string());
    let path = dir.join(&name);
    let mut file = std::fs::File::create(&path).map_err(io)?;
    let mut written = 0u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| SoundFetchError::Network(e.to_string()))?
    {
        written += chunk.len() as u64;
        if written > max_bytes {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err(SoundFetchError::TooLarge);
        }
        file.write_all(&chunk).map_err(io)?;
    }
    file.flush().map_err(io)?;
    Ok(path)
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

    #[test]
    fn classifies_sound_links() {
        let id = "1AbC-dEf_123";
        let drive = classify_sound(&format!("https://drive.google.com/file/d/{id}/view")).unwrap();
        assert_eq!(
            drive.download,
            format!("https://drive.google.com/uc?export=download&id={id}")
        );
        let one = classify_sound("https://1drv.ms/u/s!AbCdEf").unwrap();
        assert!(one
            .download
            .starts_with("https://api.onedrive.com/v1.0/shares/u!"));
        let dropbox =
            classify_sound("https://www.dropbox.com/scl/fi/abc/Walk%20in.mp3?rlkey=k&dl=0")
                .unwrap();
        assert_eq!(
            dropbox.download,
            "https://www.dropbox.com/scl/fi/abc/Walk%20in.mp3?rlkey=k&dl=1"
        );
        assert_eq!(dropbox.fallback_name, "Walk in.mp3");
        let plain = classify_sound(" example.org/music/Be%20Thou.FLAC ").unwrap();
        assert_eq!(plain.download, "https://example.org/music/Be%20Thou.FLAC");
        assert_eq!(plain.fallback_name, "Be Thou.FLAC");
    }

    #[test]
    fn refuses_streaming_and_other_links() {
        for url in [
            "https://www.youtube.com/watch?v=abc",
            "https://youtu.be/abc",
            "https://music.youtube.com/watch?v=abc",
            "https://open.spotify.com/track/abc",
            "https://music.apple.com/us/album/x/1",
            "https://soundcloud.com/artist/track",
            "https://on.soundcloud.com/abc",
            // Even a link that ends like a file.
            "https://soundcloud.com/artist/track.mp3",
        ] {
            assert_eq!(classify_sound(url), Err(SoundLinkError::Streaming), "{url}");
        }
        for url in [
            "https://example.org/page",
            "https://example.org/deck.pptx",
            "http://example.org/track.mp3",
            "ftp://example.org/track.mp3",
            "https://docs.google.com/presentation/d/abc/edit",
            "https://www.dropbox.com/sh/abc/def?dl=0",
            "not a link",
        ] {
            assert_eq!(
                classify_sound(url),
                Err(SoundLinkError::Unsupported),
                "{url}"
            );
        }
        // Not fooled by a host that merely ends the same way.
        assert!(classify_sound("https://notyoutube.com/a.mp3").is_ok());
    }

    const MP3: &[u8] = b"ID3\x03\x00\x00fake-mp3-bytes";

    fn sound_link(url: String, fallback: &str) -> SoundLink {
        SoundLink {
            download: url,
            fallback_name: fallback.into(),
        }
    }

    #[tokio::test]
    async fn fetch_sound_streams_to_a_named_file() {
        let (_server, url) = serve(
            ResponseTemplate::new(200)
                .insert_header("content-type", "audio/mpeg")
                .insert_header(
                    "content-disposition",
                    "attachment; filename=\"Walk in.mp3\"",
                )
                .set_body_bytes(MP3),
        )
        .await;
        let dir = tempfile::tempdir().unwrap();
        let path = fetch_sound_limited(&sound_link(url, "other.mp3"), dir.path(), 1024)
            .await
            .unwrap();
        assert_eq!(path, dir.path().join("Walk in.mp3"));
        assert_eq!(std::fs::read(path).unwrap(), MP3);
    }

    #[tokio::test]
    async fn fetch_sound_names_the_file_from_the_link_and_stays_in_the_folder() {
        let (_server, url) = serve(ResponseTemplate::new(200).set_body_bytes(MP3)).await;
        let dir = tempfile::tempdir().unwrap();
        let path = fetch_sound_limited(&sound_link(url.clone(), "From link.mp3"), dir.path(), 1024)
            .await
            .unwrap();
        assert_eq!(path, dir.path().join("From link.mp3"));
        // No name at all.
        let path = fetch_sound_limited(&sound_link(url.clone(), ""), dir.path(), 1024)
            .await
            .unwrap();
        assert_eq!(path, dir.path().join("Track"));
        // A name that tries to leave the folder.
        let (_server, url) = serve(
            ResponseTemplate::new(200)
                .insert_header(
                    "content-disposition",
                    "attachment; filename=\"../../evil.mp3\"",
                )
                .set_body_bytes(MP3),
        )
        .await;
        let path = fetch_sound_limited(&sound_link(url, ""), dir.path(), 1024)
            .await
            .unwrap();
        assert_eq!(path, dir.path().join("evil.mp3"));
    }

    #[tokio::test]
    async fn fetch_sound_refusals() {
        let dir = tempfile::tempdir().unwrap();
        let (_s, url) = serve(ResponseTemplate::new(403)).await;
        assert!(matches!(
            fetch_sound_limited(&sound_link(url, "a.mp3"), dir.path(), 1024).await,
            Err(SoundFetchError::Forbidden)
        ));
        let (_s, url) = serve(ResponseTemplate::new(404)).await;
        assert!(matches!(
            fetch_sound_limited(&sound_link(url, "a.mp3"), dir.path(), 1024).await,
            Err(SoundFetchError::NotFound)
        ));
        let (_s, url) = serve(
            ResponseTemplate::new(200)
                .set_body_raw("<html>Sign in</html>", "text/html; charset=utf-8"),
        )
        .await;
        assert!(matches!(
            fetch_sound_limited(&sound_link(url, "a.mp3"), dir.path(), 1024).await,
            Err(SoundFetchError::WebPage)
        ));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn fetch_sound_stops_at_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let (_s, url) = serve(ResponseTemplate::new(200).set_body_bytes(vec![7u8; 5000])).await;
        let result = fetch_sound_limited(&sound_link(url, "big.mp3"), dir.path(), 4096).await;
        assert!(matches!(result, Err(SoundFetchError::TooLarge)));
        // Nothing is left behind.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        // Exactly at the limit is fine.
        let (_s, url) = serve(ResponseTemplate::new(200).set_body_bytes(vec![7u8; 4096])).await;
        assert!(
            fetch_sound_limited(&sound_link(url, "ok.mp3"), dir.path(), 4096)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn streaming_links_make_no_request() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        // Refused before any link is built, so there is nothing to fetch.
        for url in [
            "https://www.youtube.com/watch?v=abc",
            "https://open.spotify.com/track/abc",
            "https://soundcloud.com/a/b",
        ] {
            assert!(classify_sound(url).is_err());
        }
        assert!(server.received_requests().await.unwrap().is_empty());
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
