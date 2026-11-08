//! The Notion page reader against pages shaped like Mona Heights Chapel's
//! service plans (8 November and 23 August 2026), in the API's JSON shape.
//! Names, phone numbers and meeting IDs are replaced: this repository is
//! public. T112 replaces these with captured responses.

use lampstand_connectors::notion::page::{page_to_plan, ReadOptions, ReadWarning};
use lampstand_connectors::notion::{block_from_api, page_meta, NotionClient, NotionError};
use lampstand_core::plan::{EntryType, Plan, PlanStatus};
use serde_json::{json, Value};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn block(id: &str, kind: &str, text: &str) -> Value {
    json!({
        "object": "block",
        "id": id,
        "type": kind,
        "has_children": false,
        kind: { "rich_text": [ { "type": "text", "plain_text": text, "text": { "content": text } } ] }
    })
}

fn page(id: &str, title: &str) -> Value {
    json!({
        "object": "page",
        "id": id,
        "last_edited_time": "2026-11-08T05:12:00.000Z",
        "properties": { "title": { "id": "title", "type": "title",
            "title": [ { "plain_text": title } ] } }
    })
}

/// The 8 November plan, top to bottom.
fn november() -> Vec<Value> {
    let rows: &[(&str, &str)] = &[
        ("callout", "🟡"),
        ("paragraph", "Date: 11/08/2026\nStatus: Draft"),
        ("divider", ""),
        ("heading_1", "MONA HEIGHTS CHAPEL"),
        ("heading_2", "FAMILY BIBLE HOUR"),
        ("paragraph", "CHAIRPERSON: Ada Example"),
        ("heading_2", "ORDER OF SERVICE"),
        ("heading_3", "10:00 am – Welcome and Opening Prayer"),
        (
            "callout",
            "Please be guided that the sermons are broadcast via ZOOM and YouTube live.",
        ),
        ("heading_3", "Opening Remarks"),
        ("bulleted_list_item", "Welcome to persons"),
        ("bulleted_list_item", "Comment on Last Week's Sermon:"),
        (
            "quote",
            "“For I consider that the sufferings of this present time…” (Romans 8:18, NKJV)",
        ),
        (
            "bulleted_list_item",
            "Invite Persons into a Place of Worship",
        ),
        ("heading_3", "10:05 am – Opening Songs"),
        (
            "bulleted_list_item",
            "Opening Song: Come Thou Almighty King (C major)",
        ),
        ("bulleted_list_item", "Congregational Song: TBD 🔶"),
        ("heading_3", "10:10 am – Opening Prayer"),
        (
            "paragraph",
            "Choose Someone from Congregation to Pray at the Microphone",
        ),
        ("heading_3", "10:15 am – First Scripture Reading"),
        ("paragraph", "TBD 🔶"),
        (
            "callout",
            "Note for Chairman: Have the church read alternately.",
        ),
        ("heading_3", "10:20 am – Praise and Worship"),
        ("numbered_list_item", "Indescribable"),
        ("numbered_list_item", "I Stand in Awe of You"),
        (
            "numbered_list_item",
            "I Exalt You (Thou Art High, O Lord, Above All the Earth)",
        ),
        ("heading_3", "Statement Before the Breaking of Bread"),
        ("callout", "As we approach the Lord's Table this morning…"),
        ("heading_3", "10:35 am – Breaking of Bread"),
        ("paragraph", "Leader: TBD"),
        ("bulleted_list_item", "Communion Passage: TBD"),
        ("bulleted_list_item", "Communion Hymn: TBD"),
        ("heading_3", "10:55 am – Collection of Offering"),
        ("paragraph", "Offertory Hymn: TBD 🔶"),
        ("heading_3", "11:05 am – Second Welcome and Announcements"),
        ("heading_2", "Announcements"),
        ("heading_3", "Birthdays (November 8 – November 14)"),
        ("bulleted_list_item", "Nov 9 – Bea Sample"),
        ("bulleted_list_item", "Nov 12 – Cy Placeholder"),
        ("heading_3", "Prayer Diary"),
        ("bulleted_list_item", "Sundays: Mona families"),
        ("heading_3", "Weekly Schedule"),
        (
            "bulleted_list_item",
            "Sunday 10:00 AM – Sunday Service. Zoom ID: 000 0000 0000",
        ),
        ("heading_3", "11:12 am – Second Scripture Reading"),
        (
            "paragraph",
            "Romans 8:18-21 (NKJV) — est. 1 minute, read alternately",
        ),
        (
            "paragraph",
            "Note for Chairman: Have the church read alternately.",
        ),
        ("heading_3", "11:16 am – Special Notices"),
        ("paragraph", "Men's Fellowship — Saturday, November 14"),
        ("heading_3", "11:23 am – Children's Church"),
        ("paragraph", "Volunteers are needed to lead this ministry."),
        ("heading_3", "11:27 am – Third Scripture Reading"),
        (
            "paragraph",
            "Romans 8:22-25 (NKJV) — est. 1 minute, read alternately",
        ),
        ("heading_3", "11:30 am – Sermon"),
        (
            "paragraph",
            "Sermon Title: “All the Way to Glory” Speaker: Dr. Dee Visitor",
        ),
        ("quote", "“For I consider…” (Romans 8:18, NKJV)"),
        (
            "paragraph",
            "Dr. Visitor will speak from Romans 8:18-25, focusing on God's plan.",
        ),
        ("heading_3", "12:05 pm – Call for Response and closing song"),
        ("callout", "TBA"),
        ("paragraph", "Benediction"),
    ];
    rows.iter()
        .enumerate()
        .map(|(i, (k, t))| block(&format!("b{i:02}"), k, t))
        .collect()
}

fn read(title: &str, rows: Vec<Value>) -> (Plan, Vec<ReadWarning>) {
    let meta = page_meta(&page("4127e73a-e67c-813c-933e-eddf604829ae", title));
    let blocks: Vec<_> = rows.iter().filter_map(block_from_api).collect();
    let options = ReadOptions {
        time_zone: Some("America/Jamaica".into()),
        utc_offset_minutes: Some(-300),
    };
    page_to_plan(&meta, &blocks, &options).unwrap()
}

#[test]
fn reads_the_november_plan() {
    let (plan, warnings) = read("Service Plan – November 8, 2026 – Ada Example", november());
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(plan.title, "Service Plan – November 8, 2026");
    assert_eq!(plan.starts_at.as_deref(), Some("2026-11-08T10:00:00-05:00"));
    assert_eq!(plan.time_zone.as_deref(), Some("America/Jamaica"));
    assert_eq!(plan.status, Some(PlanStatus::Draft));
    assert_eq!(plan.source.kind, "notion");
    assert_eq!(
        plan.source.edited.as_deref(),
        Some("2026-11-08T05:12:00.000Z")
    );

    let got: Vec<(EntryType, &str)> = plan
        .ordered()
        .iter()
        .map(|e| (e.entry_type, e.title.as_str()))
        .collect();
    use EntryType::*;
    assert_eq!(
        got,
        [
            (Header, "Welcome and Opening Prayer"),
            (Header, "Opening Songs"),
            (Song, "Come Thou Almighty King"),
            (Other, "Congregational Song"),
            (Header, "Opening Prayer"),
            (Header, "First Scripture Reading"),
            (Other, "Reading"),
            (Header, "Praise and Worship"),
            (Song, "Indescribable"),
            (Song, "I Stand in Awe of You"),
            (
                Song,
                "I Exalt You (Thou Art High, O Lord, Above All the Earth)"
            ),
            (Header, "Breaking of Bread"),
            (Other, "Communion Passage"),
            (Other, "Communion Hymn"),
            (Header, "Collection of Offering"),
            (Other, "Offertory Hymn"),
            (Announcement, "Second Welcome and Announcements"),
            (Header, "Second Scripture Reading"),
            (Scripture, "Romans 8:18–21"),
            (Announcement, "Special Notices"),
            (Header, "Children's Church"),
            (Header, "Third Scripture Reading"),
            (Scripture, "Romans 8:22–25"),
            (Slides, "Sermon"),
            (Header, "Call for Response and closing song"),
            (Other, "Song"),
        ]
    );

    let opening = &plan.entries[2];
    assert_eq!(opening.key.as_deref(), Some("C"));
    assert_eq!(
        opening.song.as_ref().unwrap().title,
        "Come Thou Almighty King"
    );
    let reading = plan
        .entries
        .iter()
        .find(|e| e.entry_type == Scripture)
        .unwrap();
    assert_eq!(reading.translation.as_deref(), Some("NKJV"));
    assert_eq!(
        plan.entries[3].notes.as_deref(),
        Some("Not chosen yet in Notion")
    );
    assert_eq!(
        plan.entries[1].notes.as_deref(),
        Some("Planned for 10:05 am")
    );

    // The plan is valid by the contract and survives a round trip.
    let (again, _) = Plan::from_json(&plan.to_json()).unwrap();
    assert_eq!(again, plan);
}

#[test]
fn nothing_about_people_reaches_the_plan() {
    let (plan, _) = read("Service Plan – November 8, 2026 – Ada Example", november());
    let json = plan.to_json();
    for name in [
        "Ada",
        "Bea",
        "Cy ",
        "Dee",
        "Visitor",
        "Zoom",
        "Mona families",
        "Volunteers",
        "CHAIRPERSON",
    ] {
        assert!(!json.contains(name), "{name} leaked into the plan");
    }
}

#[test]
fn reads_the_august_plan() {
    // Its first reading is "TBD" in the heading, it lists the communion
    // passages on one line, and a section was moved without its time.
    let rows: Vec<Value> = [
        ("heading_2", "ORDER OF SERVICE"),
        ("heading_3", "10:00 am – Welcome and Opening Prayer"),
        ("heading_3", "10:15 am – First Scripture Reading: TBD"),
        ("heading_3", "10:05 am – Opening Songs"),
        (
            "bulleted_list_item",
            "Opening Song: All Hail the Power of Jesus' Name",
        ),
        (
            "bulleted_list_item",
            "Congregational Song: Lord I Lift Your Name on High",
        ),
        ("heading_3", "10:20 am – Breaking of Bread"),
        (
            "bulleted_list_item",
            "Communion Passages: Mark 14:12-26; Exodus 12:7-20 (NIV)",
        ),
        (
            "bulleted_list_item",
            "Communion Hymn: Man of Sorrows, What a Name",
        ),
    ]
    .iter()
    .enumerate()
    .map(|(i, (k, t))| block(&format!("a{i}"), k, t))
    .collect();
    let (plan, warnings) = read("Service Plan – August 23, 2026 – Ada Example", rows);
    assert_eq!(
        warnings,
        [ReadWarning::TimeOutOfOrder {
            heading: "10:05 am – Opening Songs".into()
        }]
    );
    assert_eq!(plan.starts_at.as_deref(), Some("2026-08-23T10:00:00-05:00"));
    assert_eq!(plan.status, None);

    let titles: Vec<&str> = plan.ordered().iter().map(|e| e.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Welcome and Opening Prayer",
            "First Scripture Reading",
            "Reading",
            "Opening Songs",
            "All Hail the Power of Jesus' Name",
            "Lord I Lift Your Name on High",
            "Breaking of Bread",
            "Mark 14:12–26",
            "Exodus 12:7–20",
            "Man of Sorrows, What a Name",
        ]
    );
    let exodus = plan
        .entries
        .iter()
        .find(|e| e.title.starts_with("Exodus"))
        .unwrap();
    assert_eq!(exodus.translation.as_deref(), Some("NIV"));
    assert_eq!(exodus.source_id, "a7#2");
}

#[test]
fn source_ids_are_unique_and_stable() {
    let (a, _) = read("Service Plan – November 8, 2026 – Ada Example", november());
    let (b, _) = read("Service Plan – November 8, 2026 – Ada Example", november());
    let ids: Vec<_> = a.entries.iter().map(|e| &e.source_id).collect();
    assert_eq!(
        ids,
        b.entries.iter().map(|e| &e.source_id).collect::<Vec<_>>()
    );
    a.validate().unwrap();
}

#[test]
fn without_an_offset_there_is_no_start_time() {
    let meta = page_meta(&page("p", "Service Plan – November 8, 2026"));
    let blocks: Vec<_> = november().iter().filter_map(block_from_api).collect();
    let (plan, warnings) = page_to_plan(&meta, &blocks, &ReadOptions::default()).unwrap();
    assert_eq!(plan.starts_at, None);
    assert_eq!(warnings, [ReadWarning::NoTimeZone]);
}

#[test]
fn a_page_with_no_timed_headings_is_refused() {
    let meta = page_meta(&page("p", "Meeting notes"));
    let blocks: Vec<_> = [block("x", "paragraph", "Hello")]
        .iter()
        .filter_map(block_from_api)
        .collect();
    assert!(page_to_plan(&meta, &blocks, &ReadOptions::default()).is_err());
}

// --- The client, against a mock API.

const PAGE: &str = "4127e73a-e67c-813c-933e-eddf604829ae";

#[tokio::test]
async fn follows_the_cursor_and_nested_blocks() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/pages/{PAGE}")))
        .and(header("authorization", "Bearer secret_t"))
        .and(header("notion-version", "2025-09-03"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(page(PAGE, "Service Plan – November 8, 2026")),
        )
        .mount(&server)
        .await;
    let children = format!("/v1/blocks/{PAGE}/children");
    for (cursor, next, id) in [
        (None, Some("c2"), "one"),
        (Some("c2"), Some("c3"), "two"),
        (Some("c3"), None, "three"),
    ] {
        let mut b = block(id, "heading_3", id);
        if id == "two" {
            b["has_children"] = json!(true);
        }
        let body = json!({ "results": [b], "has_more": next.is_some(), "next_cursor": next });
        let mut mock = Mock::given(method("GET")).and(path(children.clone()));
        if let Some(c) = cursor {
            mock = mock.and(query_param("start_cursor", c));
        }
        mock.respond_with(ResponseTemplate::new(200).set_body_json(body))
            .with_priority(if cursor.is_some() { 1 } else { 5 })
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/v1/blocks/two/children"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [block("two-a", "numbered_list_item", "Indescribable")], "has_more": false, "next_cursor": null
        })))
        .mount(&server)
        .await;

    let client = NotionClient::with_base("secret_t", server.uri());
    let link =
        "https://app.notion.com/p/Service-Plan-November-8-2026-4127e73ae67c813c933eeddf604829ae";
    let (meta, blocks) = client.read_page(link).await.unwrap();
    assert_eq!(meta.title, "Service Plan – November 8, 2026");
    let ids: Vec<&str> = blocks.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, ["one", "two", "three"]);
    assert_eq!(blocks[1].children[0].text, "Indescribable");
}

#[tokio::test]
async fn retries_after_a_rate_limit() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/pages/{PAGE}")))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/pages/{PAGE}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(page(PAGE, "T")))
        .with_priority(2)
        .mount(&server)
        .await;
    let client = NotionClient::with_base("secret_t", server.uri());
    assert_eq!(client.page(PAGE).await.unwrap().title, "T");
}

#[tokio::test]
async fn gives_up_after_three_rate_limits() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
        .expect(3)
        .mount(&server)
        .await;
    let client = NotionClient::with_base("secret_t", server.uri());
    assert_eq!(
        client.page(PAGE).await.unwrap_err(),
        NotionError::RateLimited
    );
}

#[tokio::test]
async fn names_a_bad_token_and_an_unshared_page() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/pages/bad"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({ "code": "unauthorized" })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/pages/hidden"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(json!({ "code": "object_not_found" })),
        )
        .mount(&server)
        .await;
    let client = NotionClient::with_base("secret_never_shown", server.uri());
    let unauthorized = client.page("bad").await.unwrap_err();
    let hidden = client.page("hidden").await.unwrap_err();
    assert_eq!(unauthorized, NotionError::Unauthorized);
    assert_eq!(hidden, NotionError::NotShared);
    for e in [unauthorized, hidden] {
        assert!(!format!("{e:?} {e}").contains("secret_never_shown"));
    }
}

// --- A page shared to the web, read from its link alone.

use lampstand_connectors::notion::public::PublicReader;

fn record(id: &str, kind: &str, text: &str, content: &[&str]) -> Value {
    json!({ "spaceId": "s", "value": { "value": {
        "id": id, "type": kind, "alive": true,
        "properties": { "title": [[text]] },
        "content": content,
        "last_edited_time": 1_794_106_324_711_i64
    }, "role": "reader" } })
}

#[tokio::test]
async fn reads_a_public_page_across_chunks() {
    let server = MockServer::start().await;
    let first = json!({
        "cursor": { "stack": [[{ "table": "block", "id": PAGE, "index": 2 }]] },
        "recordMap": { "block": {
            PAGE: record(PAGE, "page", "Service Plan – November 8, 2026 – Ada Example", &["h1", "s1", "w1", "sub"]),
            "h1": record("h1", "sub_header", "ORDER OF SERVICE", &[]),
            "s1": record("s1", "sub_sub_header", "10:05 am – Opening Songs", &[]),
        } }
    });
    let second = json!({
        "cursor": { "stack": [] },
        "recordMap": { "block": {
            "w1": record("w1", "bulleted_list", "Opening Song: Come Thou Almighty King (C major)", &[]),
            "sub": record("sub", "page", "A child page", &[]),
        } }
    });
    Mock::given(method("POST"))
        .and(path("/api/v3/loadPageChunk"))
        .and(wiremock::matchers::body_partial_json(
            json!({ "chunkNumber": 0 }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(first))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v3/loadPageChunk"))
        .and(wiremock::matchers::body_partial_json(
            json!({ "chunkNumber": 1 }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(second))
        .mount(&server)
        .await;

    let reader = PublicReader::with_base(server.uri());
    let link = "https://app.notion.com/p/Service-Plan-4127e73ae67c813c933eeddf604829ae";
    let (meta, blocks) = reader.read_page(link).await.unwrap();
    assert_eq!(
        meta.last_edited.as_deref(),
        Some("2026-11-08T02:52:04.711Z")
    );
    assert_eq!(blocks.len(), 3, "the child page is left out");
    let options = ReadOptions {
        time_zone: None,
        utc_offset_minutes: Some(-300),
    };
    let (plan, _) = page_to_plan(&meta, &blocks, &options).unwrap();
    assert_eq!(plan.starts_at.as_deref(), Some("2026-11-08T10:05:00-05:00"));
    assert_eq!(plan.entries[1].title, "Come Thou Almighty King");
}

#[tokio::test]
async fn a_private_page_says_how_to_publish_it() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "cursor": { "stack": [] }, "recordMap": { "__version__": 3 }
        })))
        .mount(&server)
        .await;
    let reader = PublicReader::with_base(server.uri());
    let err = reader.read_page(PAGE).await.unwrap_err();
    assert_eq!(err, NotionError::NotPublic);
    assert!(err.to_string().contains("Publish to web"));
}

/// The chapel's real 8 November page. Needs the network:
/// `cargo test -p lampstand-connectors --features notion --test notion -- --ignored`
#[tokio::test]
#[ignore = "reads the live page from notion.so"]
async fn reads_the_live_november_page() {
    let link = "https://app.notion.com/p/Service-Plan-November-8-2026-Daniel-Fokum-4127e73ae67c813c933eeddf604829ae";
    let (meta, blocks) = PublicReader::default().read_page(link).await.unwrap();
    let options = ReadOptions {
        time_zone: Some("America/Jamaica".into()),
        utc_offset_minutes: Some(-300),
    };
    let (plan, warnings) = page_to_plan(&meta, &blocks, &options).unwrap();
    for e in plan.ordered() {
        println!(
            "{:>3} {:<12} {}",
            e.order,
            format!("{:?}", e.entry_type),
            e.title
        );
    }
    println!("warnings: {warnings:?}");
    assert_eq!(plan.title, "Service Plan – November 8, 2026");
    assert_eq!(plan.starts_at.as_deref(), Some("2026-11-08T10:00:00-05:00"));
    assert!(plan
        .entries
        .iter()
        .any(|e| e.title == "Come Thou Almighty King"));
    assert!(plan.entries.iter().any(|e| e.title == "Song"));
    assert!(!plan.to_json().contains("Fokum"));
}
