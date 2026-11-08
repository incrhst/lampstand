//! Reads a Notion page that is shared to the web, from its link alone: no
//! integration, no token, no setup.
//!
//! This is the endpoint Notion's own web page uses to show a public page
//! (`POST /api/v3/loadPageChunk`). It is not part of Notion's published
//! API and may change without notice, so it sits behind this one module
//! and the official client ([`super::NotionClient`]) stays as the fallback
//! for private pages. A private page comes back empty, which is reported as
//! [`NotionError::NotPublic`].
//!
//! The answer is a flat map of block records, each listing its children's
//! IDs in `content`. Chunks are read until the cursor is empty.

use std::collections::HashMap;
use std::time::Duration;

use reqwest::header::{CONTENT_TYPE, RETRY_AFTER};
use reqwest::StatusCode;
use serde_json::{json, Value};

use super::page::{Block, BlockKind, PageMeta};
use super::{page_id_from_link, NotionError};

pub const PUBLIC_BASE: &str = "https://www.notion.so";
const TRIES: u32 = 3;
/// A plan is a few hundred blocks; this stops a runaway cursor.
const MAX_CHUNKS: u32 = 20;
const MAX_DEPTH: u8 = 6;

#[derive(Debug, Clone)]
pub struct PublicReader {
    http: reqwest::Client,
    base: String,
}

impl Default for PublicReader {
    fn default() -> Self {
        Self::with_base(PUBLIC_BASE)
    }
}

impl PublicReader {
    /// For tests: a different origin.
    pub fn with_base(base: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: base.into().trim_end_matches('/').to_string(),
        }
    }

    /// The page and its blocks, from a pasted link.
    pub async fn read_page(&self, link: &str) -> Result<(PageMeta, Vec<Block>), NotionError> {
        let id = page_id_from_link(link).ok_or(NotionError::NotAPageLink)?;
        let mut records: HashMap<String, Value> = HashMap::new();
        let mut cursor = json!({ "stack": [] });
        for chunk in 0..MAX_CHUNKS {
            let body = json!({
                "pageId": id,
                "limit": 100,
                "cursor": cursor,
                "chunkNumber": chunk,
                "verticalColumns": false,
            });
            let answer = self.post("/api/v3/loadPageChunk", &body).await?;
            if let Some(blocks) = answer["recordMap"]["block"].as_object() {
                for (k, v) in blocks {
                    records.insert(k.clone(), record_value(v).clone());
                }
            }
            cursor = answer["cursor"].clone();
            if cursor["stack"].as_array().is_none_or(Vec::is_empty) {
                break;
            }
        }
        from_records(&id, &records)
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value, NotionError> {
        let url = format!("{}{path}", self.base);
        for attempt in 1..=TRIES {
            let response = self
                .http
                .post(&url)
                .header(CONTENT_TYPE, "application/json")
                .body(body.to_string())
                .send()
                .await
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
            return if status.is_success() {
                Ok(json)
            } else {
                Err(NotionError::Unexpected {
                    status: status.as_u16(),
                    code: json["name"].as_str().map(String::from),
                })
            };
        }
        Err(NotionError::RateLimited)
    }
}

/// A record is `{ "value": { "value": block, "role": … } }` in recent
/// answers and `{ "value": block }` in older ones.
fn record_value(record: &Value) -> &Value {
    let outer = &record["value"];
    if outer["value"].is_object() {
        &outer["value"]
    } else {
        outer
    }
}

/// Builds the page from the block records of one or more chunks. Blocks
/// keyed by ID, each already unwrapped to the block itself.
pub fn from_records(
    page_id: &str,
    records: &HashMap<String, Value>,
) -> Result<(PageMeta, Vec<Block>), NotionError> {
    let root = records
        .get(page_id)
        .filter(|r| r["type"] == "page")
        .ok_or(NotionError::NotPublic)?;
    let meta = PageMeta {
        id: page_id.to_string(),
        title: title_text(root),
        last_edited: root["last_edited_time"].as_i64().map(rfc3339_from_millis),
    };
    Ok((meta, children(root, records, 0)))
}

fn children(parent: &Value, records: &HashMap<String, Value>, depth: u8) -> Vec<Block> {
    if depth > MAX_DEPTH {
        return Vec::new();
    }
    let ids = parent["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str);
    let mut out = Vec::new();
    for id in ids {
        let Some(record) = records.get(id) else {
            continue;
        };
        if record["alive"] == false {
            continue;
        }
        let kind_name = record["type"].as_str().unwrap_or_default();
        let kind = match kind_name {
            "header" => BlockKind::Heading(1),
            "sub_header" => BlockKind::Heading(2),
            "sub_sub_header" => BlockKind::Heading(3),
            "text" => BlockKind::Paragraph,
            "bulleted_list" => BlockKind::Bulleted,
            "numbered_list" => BlockKind::Numbered,
            "to_do" => BlockKind::ToDo,
            "toggle" => BlockKind::Toggle,
            "quote" => BlockKind::Quote,
            "callout" => BlockKind::Callout,
            "divider" => BlockKind::Divider,
            // A child page is another page, not part of this one.
            "page" => continue,
            other => BlockKind::Other(other.to_string()),
        };
        let nested = match kind {
            BlockKind::Quote => Vec::new(),
            _ => children(record, records, depth + 1),
        };
        out.push(Block {
            id: id.to_string(),
            kind,
            text: title_text(record),
            children: nested,
        });
    }
    out
}

/// `properties.title` is `[["text", [formats]], …]`; a mention is "‣".
fn title_text(record: &Value) -> String {
    record["properties"]["title"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|run| run[0].as_str())
        .filter(|t| *t != "‣")
        .collect()
}

/// Milliseconds since 1970 as `2026-11-08T05:12:00.000Z`.
fn rfc3339_from_millis(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_times() {
        assert_eq!(rfc3339_from_millis(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            rfc3339_from_millis(1_794_106_324_711),
            "2026-11-08T02:52:04.711Z"
        );
        assert_eq!(
            rfc3339_from_millis(951_782_400_000),
            "2000-02-29T00:00:00.000Z"
        );
    }

    #[test]
    fn unwraps_both_record_shapes() {
        let new = json!({ "value": { "value": { "id": "a" }, "role": "reader" } });
        let old = json!({ "value": { "id": "a" } });
        assert_eq!(record_value(&new)["id"], "a");
        assert_eq!(record_value(&old)["id"], "a");
    }
}
