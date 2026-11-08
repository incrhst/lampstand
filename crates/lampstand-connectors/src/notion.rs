//! Notion (`notion` feature, decision 0007): reads a service plan page.
//!
//! Two ways in. A page shared to the web reads from its link alone
//! ([`public::PublicReader`]); try that first. A private page needs the
//! official API with an internal integration token ([`NotionClient`]).
//!
//! The church creates an integration in its own workspace and shares the
//! service plans page with it; that sharing is all Lampstand can see.
//! Lampstand only reads. The token is never logged, never put in an error
//! and never shown by `Debug`.
//!
//! API: `GET /v1/pages/{id}` for the title and last-edited time, and `GET
//! /v1/blocks/{id}/children` (paged, 100 a page) for the content, following
//! children of lists, toggles and columns. Notion averages about three
//! requests a second; a 429 is retried after its `Retry-After`, three
//! times. The `Notion-Version` is pinned here, so a version change is this
//! file.

pub mod page;
pub mod public;

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use reqwest::header::{AUTHORIZATION, RETRY_AFTER};
use reqwest::StatusCode;
use serde_json::Value;

use page::{Block, BlockKind, PageMeta};

pub const NOTION_VERSION: &str = "2025-09-03";
pub const API_BASE: &str = "https://api.notion.com";
const TRIES: u32 = 3;
/// How deep nested blocks are followed. A plan's lists nest once or twice.
const MAX_DEPTH: u8 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotionError {
    /// The token was refused.
    Unauthorized,
    /// Notion answers as if the page did not exist when the integration
    /// has not been given it, so this usually means "share the page".
    NotShared,
    RateLimited,
    Network(String),
    /// Any other answer. Carries the status and Notion's own code, never
    /// the token.
    Unexpected {
        status: u16,
        code: Option<String>,
    },
    /// The link does not hold a Notion page ID.
    NotAPageLink,
    /// Read from its link alone, the page came back empty: it is not shared
    /// to the web (or does not exist).
    NotPublic,
}

impl std::fmt::Display for NotionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized => write!(f, "Notion did not accept the integration token. Check it in Settings › Connections."),
            Self::NotShared => write!(
                f,
                "Notion could not find that page for this integration. In Notion, open the page, choose ••• › Connections, and add the Lampstand integration."
            ),
            Self::RateLimited => write!(f, "Notion is busy. Wait a minute and try again."),
            Self::Network(why) => write!(f, "Could not reach Notion: {why}"),
            Self::Unexpected { status, code } => match code {
                Some(code) => write!(f, "Notion answered {status} ({code})."),
                None => write!(f, "Notion answered {status}."),
            },
            Self::NotAPageLink => write!(f, "That is not a link to a Notion page."),
            Self::NotPublic => write!(
                f,
                "Lampstand could not read that page from its link. In Notion, open the page, choose Share › Publish, and turn on \"Publish to web\". Or connect a Notion integration in Settings › Connections."
            ),
        }
    }
}

impl std::error::Error for NotionError {}

pub struct NotionClient {
    http: reqwest::Client,
    token: String,
    base: String,
}

impl std::fmt::Debug for NotionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotionClient")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl NotionClient {
    pub fn new(token: impl Into<String>) -> Self {
        Self::with_base(token, API_BASE)
    }

    /// For tests: a different API origin.
    pub fn with_base(token: impl Into<String>, base: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            token: token.into(),
            base: base.into().trim_end_matches('/').to_string(),
        }
    }

    /// The page's title and last-edited time.
    pub async fn page(&self, page_id: &str) -> Result<PageMeta, NotionError> {
        let json = self.get(&format!("/v1/pages/{page_id}")).await?;
        Ok(page_meta(&json))
    }

    /// Every block on the page, nested blocks under their parents.
    pub async fn blocks(&self, page_id: &str) -> Result<Vec<Block>, NotionError> {
        self.children(page_id.to_string(), 0).await
    }

    /// The page and its blocks: what `page::page_to_plan` reads.
    pub async fn read_page(&self, link_or_id: &str) -> Result<(PageMeta, Vec<Block>), NotionError> {
        let id = page_id_from_link(link_or_id).ok_or(NotionError::NotAPageLink)?;
        let meta = self.page(&id).await?;
        let blocks = self.blocks(&id).await?;
        Ok((meta, blocks))
    }

    fn children(
        &self,
        id: String,
        depth: u8,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Block>, NotionError>> + Send + '_>> {
        Box::pin(async move {
            let mut out = Vec::new();
            let mut cursor: Option<String> = None;
            loop {
                let mut path = format!("/v1/blocks/{id}/children?page_size=100");
                if let Some(c) = &cursor {
                    path.push_str("&start_cursor=");
                    path.push_str(
                        &url::form_urlencoded::byte_serialize(c.as_bytes()).collect::<String>(),
                    );
                }
                let json = self.get(&path).await?;
                for item in json["results"].as_array().into_iter().flatten() {
                    let Some(mut block) = block_from_api(item) else {
                        continue;
                    };
                    let follow = item["has_children"].as_bool() == Some(true)
                        && depth < MAX_DEPTH
                        && block.kind != BlockKind::Quote
                        && !matches!(&block.kind, BlockKind::Other(t) if t == "child_page" || t == "child_database");
                    if follow {
                        block.children = self.children(block.id.clone(), depth + 1).await?;
                    }
                    out.push(block);
                }
                match (json["has_more"].as_bool(), json["next_cursor"].as_str()) {
                    (Some(true), Some(next)) => cursor = Some(next.to_string()),
                    _ => break,
                }
            }
            Ok(out)
        })
    }

    async fn get(&self, path: &str) -> Result<Value, NotionError> {
        let url = format!("{}{path}", self.base);
        for attempt in 1..=TRIES {
            let response = self
                .http
                .get(&url)
                .header(AUTHORIZATION, format!("Bearer {}", self.token))
                .header("Notion-Version", NOTION_VERSION)
                .send()
                .await
                // reqwest's error can hold the URL, never the header.
                .map_err(|e| NotionError::Network(e.without_url().to_string()))?;
            let status = response.status();
            if status == StatusCode::TOO_MANY_REQUESTS {
                if attempt == TRIES {
                    return Err(NotionError::RateLimited);
                }
                let wait = response
                    .headers()
                    .get(RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<f64>().ok())
                    .unwrap_or(1.0)
                    .clamp(0.0, 30.0);
                tokio::time::sleep(Duration::from_secs_f64(wait)).await;
                continue;
            }
            let bytes = response
                .bytes()
                .await
                .map_err(|e| NotionError::Network(e.without_url().to_string()))?;
            let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            return match status {
                s if s.is_success() => Ok(json),
                StatusCode::UNAUTHORIZED => Err(NotionError::Unauthorized),
                StatusCode::NOT_FOUND => Err(NotionError::NotShared),
                // Notion answers 400 "validation_error" for an ID that is not a page.
                s => Err(NotionError::Unexpected {
                    status: s.as_u16(),
                    code: json["code"].as_str().map(String::from),
                }),
            };
        }
        Err(NotionError::RateLimited)
    }
}

/// The page ID in a pasted link, as a dashed UUID. Takes
/// `https://www.notion.so/Title-<32 hex>`, `app.notion.com/p/...`, a
/// `?p=<id>` peek link, or a bare ID with or without dashes.
pub fn page_id_from_link(link: &str) -> Option<String> {
    let link = link.trim();
    let (path, query) = link.split_once('?').unwrap_or((link, ""));
    let from_query = query.split('&').find_map(|kv| kv.strip_prefix("p="));
    let candidates = from_query
        .into_iter()
        .chain(path.trim_end_matches('/').rsplit('/').next());
    for c in candidates {
        let hex: String = c.chars().filter(|ch| *ch != '-').collect();
        if hex.len() >= 32 {
            let tail = &hex[hex.len() - 32..];
            if tail.chars().all(|ch| ch.is_ascii_hexdigit()) {
                let t = tail.to_ascii_lowercase();
                return Some(format!(
                    "{}-{}-{}-{}-{}",
                    &t[0..8],
                    &t[8..12],
                    &t[12..16],
                    &t[16..20],
                    &t[20..]
                ));
            }
        }
    }
    None
}

/// Reads a page object.
pub fn page_meta(json: &Value) -> PageMeta {
    let title = json["properties"]
        .as_object()
        .and_then(|props| props.values().find(|p| p["type"] == "title"))
        .map(|p| plain_text(&p["title"]))
        .unwrap_or_default();
    PageMeta {
        id: json["id"].as_str().unwrap_or_default().to_string(),
        title,
        last_edited: json["last_edited_time"].as_str().map(String::from),
    }
}

/// Reads one block object, without its children. `None` for something
/// that is not a block.
pub fn block_from_api(json: &Value) -> Option<Block> {
    let id = json["id"].as_str()?.to_string();
    let kind_name = json["type"].as_str()?;
    let kind = match kind_name {
        "heading_1" => BlockKind::Heading(1),
        "heading_2" => BlockKind::Heading(2),
        "heading_3" => BlockKind::Heading(3),
        "paragraph" => BlockKind::Paragraph,
        "bulleted_list_item" => BlockKind::Bulleted,
        "numbered_list_item" => BlockKind::Numbered,
        "to_do" => BlockKind::ToDo,
        "toggle" => BlockKind::Toggle,
        "quote" => BlockKind::Quote,
        "callout" => BlockKind::Callout,
        "divider" => BlockKind::Divider,
        other => BlockKind::Other(other.to_string()),
    };
    let text = plain_text(&json[kind_name]["rich_text"]);
    Some(Block {
        id,
        kind,
        text,
        children: Vec::new(),
    })
}

/// Rich text joined as plain text.
fn plain_text(rich: &Value) -> String {
    rich.as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["plain_text"].as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_page_ids_in_links() {
        let want = Some("4127e73a-e67c-813c-933e-eddf604829ae".to_string());
        assert_eq!(
            page_id_from_link("https://app.notion.com/p/Service-Plan-November-8-2026-Daniel-Fokum-4127e73ae67c813c933eeddf604829ae"),
            want
        );
        assert_eq!(
            page_id_from_link(
                "https://www.notion.so/team/Plan-4127e73ae67c813c933eeddf604829ae?pvs=4"
            ),
            want
        );
        assert_eq!(
            page_id_from_link(
                "https://www.notion.so/team/db?p=4127e73ae67c813c933eeddf604829ae&pm=s"
            ),
            want
        );
        assert_eq!(
            page_id_from_link("4127e73a-e67c-813c-933e-eddf604829ae"),
            want
        );
        assert_eq!(page_id_from_link("https://example.com/hello"), None);
    }

    #[test]
    fn debug_never_shows_the_token() {
        let client = NotionClient::new("secret_abc123");
        assert!(!format!("{client:?}").contains("secret_abc123"));
    }
}
