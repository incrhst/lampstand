//! Web pages pasted as links, such as a song's page on a publisher's site,
//! read for what is on them (`lampstand-import` finds the lyrics).

use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE};
use url::Url;

use crate::FetchError;

/// A song page is tens of kilobytes; Genius pages run to a few hundred.
const MAX_BYTES: usize = 5 * 1024 * 1024;

/// Fetches an `http(s)` page and returns its HTML. Anything that is not
/// HTML (a PDF, an image) is refused.
pub async fn fetch_page(input: &str) -> Result<String, FetchError> {
    let input = input.trim();
    let url = if input.contains("://") {
        Url::parse(input)
    } else {
        Url::parse(&format!("https://{input}"))
    }
    .map_err(|_| FetchError::Network("that is not a web address".into()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(FetchError::Network("that is not a web address".into()));
    }
    let client = reqwest::Client::builder()
        .user_agent(concat!("lampstand/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| FetchError::Network(e.to_string()))?;
    // Some sites' firewalls turn away a request that does not ask for HTML.
    let response = client
        .get(url)
        .header(ACCEPT, "text/html,application/xhtml+xml;q=0.9,*/*;q=0.8")
        .header(ACCEPT_LANGUAGE, "en;q=0.9,*;q=0.5")
        .send()
        .await
        .map_err(|e| FetchError::Network(e.to_string()))?;
    match response.status().as_u16() {
        200 => {}
        401 | 403 => return Err(FetchError::Forbidden),
        404 | 410 => return Err(FetchError::NotFound),
        status => return Err(FetchError::Network(format!("HTTP {status}"))),
    }
    let html_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_none_or(|t| t.contains("html"));
    if !html_type {
        return Err(FetchError::Network("the link is not a web page".into()));
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_BYTES as u64)
    {
        return Err(FetchError::Network("the page is too large".into()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| FetchError::Network(e.to_string()))?;
    if bytes.len() > MAX_BYTES {
        return Err(FetchError::Network("the page is too large".into()));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header_regex, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn fetches_a_page_asking_for_html() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/songs/one/"))
            .and(header_regex("accept", "text/html"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw("<html>words</html>", "text/html; charset=UTF-8"),
            )
            .mount(&server)
            .await;
        let html = fetch_page(&format!("{}/songs/one/", server.uri()))
            .await
            .unwrap();
        assert_eq!(html, "<html>words</html>");
    }

    #[tokio::test]
    async fn refuses_files_missing_pages_and_other_schemes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/lyrics.pdf"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(b"%PDF".as_slice(), "application/pdf"),
            )
            .mount(&server)
            .await;
        assert!(matches!(
            fetch_page(&format!("{}/lyrics.pdf", server.uri())).await,
            Err(FetchError::Network(_))
        ));
        assert!(matches!(
            fetch_page(&format!("{}/gone", server.uri())).await,
            Err(FetchError::NotFound)
        ));
        assert!(matches!(
            fetch_page("file:///etc/passwd").await,
            Err(FetchError::Network(_))
        ));
    }
}
