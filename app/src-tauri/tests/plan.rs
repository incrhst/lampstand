//! A service from a Notion plan, through the app (T108): read, draft,
//! review, create, and next week's draft remembering this week's picks.

use std::path::Path;
use std::sync::Arc;

use lampstand_api::{
    EntryChoice, LibraryRequest, PlanDraft, PlanRequest, Request, Response, ServiceRequest,
};
use lampstand_app::handler::{App, Placement, WindowHost};
use lampstand_connectors::notion::page::{page_to_plan, Block, BlockKind, PageMeta, ReadOptions};
use lampstand_core::plan::{Choice, Outcome, Plan, SongResult};
use lampstand_core::service::{ItemKind, Service};
use lampstand_core::song::{Song, SongId};
use lampstand_layout::Fonts;
use lampstand_render::output::DisplayInfo;
use lampstand_render::{OutputCommand, OutputSink};

struct NoOutputs;
impl OutputSink for NoOutputs {
    fn send(&self, _: lampstand_core::output::OutputId, _: OutputCommand) {}
}

struct NoWindows;
impl WindowHost for NoWindows {
    fn displays(&self) -> Vec<DisplayInfo> {
        Vec::new()
    }
    fn open(
        &mut self,
        _: lampstand_core::output::OutputId,
        _: &str,
        _: Placement,
    ) -> Result<(), String> {
        Ok(())
    }
    fn close(&mut self, _: lampstand_core::output::OutputId) {}
}

fn open(dir: &Path) -> App {
    App::open(
        dir,
        Fonts::bundled_only(),
        Arc::new(NoOutputs),
        Box::new(NoWindows),
        Arc::new(|_| {}),
    )
    .unwrap()
}

fn handle(app: &mut App, request: Request) -> Response {
    match app.handle(request) {
        Response::Error { message } => panic!("{message}"),
        r => r,
    }
}

fn put_song(app: &mut App, title: &str) -> SongId {
    let song = Song::new(title);
    let id = song.id;
    handle(app, Request::Library(LibraryRequest::PutSong(song)));
    id
}

/// The chapel's page, cut down: one song in the library, one near miss,
/// one the library lacks, a reading, and a TBD.
fn page() -> Plan {
    let b = |id: &str, kind: BlockKind, text: &str| Block {
        id: id.into(),
        kind,
        text: text.into(),
        children: vec![],
    };
    let blocks = vec![
        b("o", BlockKind::Heading(2), "ORDER OF SERVICE"),
        b("h1", BlockKind::Heading(3), "10:05 am – Opening Songs"),
        b(
            "s1",
            BlockKind::Bulleted,
            "Opening Song: Come Thou Almighty King (C major)",
        ),
        b("s2", BlockKind::Bulleted, "Congregational Song: TBD 🔶"),
        b("h2", BlockKind::Heading(3), "10:20 am – Praise and Worship"),
        b("s3", BlockKind::Numbered, "Indescribable"),
        b("s4", BlockKind::Numbered, "I Stand in Awe of You"),
        b("s5", BlockKind::Numbered, "Oceans"),
        b(
            "h3",
            BlockKind::Heading(3),
            "11:12 am – Second Scripture Reading",
        ),
        b(
            "r1",
            BlockKind::Paragraph,
            "Romans 8:18-21 (NKJV) — est. 1 minute",
        ),
    ];
    let meta = PageMeta {
        id: "4127e73a".into(),
        title: "Service Plan – November 8, 2026 – Ada Example".into(),
        last_edited: None,
    };
    let options = ReadOptions {
        time_zone: Some("America/Jamaica".into()),
        utc_offset_minutes: Some(-300),
    };
    page_to_plan(&meta, &blocks, &options).unwrap().0
}

fn draft(app: &mut App, plan: Plan) -> PlanDraft {
    match handle(app, Request::Plan(PlanRequest::DraftFromPlan { plan })) {
        Response::PlanDraft(d) => d,
        other => panic!("{other:?}"),
    }
}

fn services(app: &mut App) -> usize {
    match handle(app, Request::Service(ServiceRequest::List)) {
        Response::ServiceList(list) => list.services.len(),
        Response::Services(list) => list.len(),
        other => panic!("{other:?}"),
    }
}

fn result_of<'a>(d: &'a PlanDraft, source_id: &str) -> &'a SongResult {
    match &d
        .plan
        .entries
        .iter()
        .find(|e| e.source_id == source_id)
        .unwrap()
        .outcome
    {
        Outcome::Song { result, .. } => result,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_notion_plan_becomes_a_service() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = open(dir.path());
    let king = put_song(&mut app, "Come Thou Almighty King");
    let indescribable = put_song(&mut app, "Indescribable");
    let awe = put_song(&mut app, "I Stand in Awe");
    let before = services(&mut app);

    // The draft writes nothing.
    let d = draft(&mut app, page());
    assert_eq!(services(&mut app), before);
    assert!(d.existing.is_none());
    assert!(matches!(result_of(&d, "s1"), SongResult::Matched { song, .. } if *song == king));
    assert!(matches!(result_of(&d, "s4"), SongResult::Likely { song, .. } if *song == awe));
    assert_eq!(result_of(&d, "s5"), &SongResult::NotFound);

    // The person confirms the likely song and makes a blank one for Oceans.
    let choices = vec![
        EntryChoice {
            source_id: "s4".into(),
            choice: Choice::Song { song: awe },
        },
        EntryChoice {
            source_id: "s5".into(),
            choice: Choice::CreateBlank,
        },
    ];
    let service = match handle(
        &mut app,
        Request::Plan(PlanRequest::CreateFromPlan {
            draft: d.plan.clone(),
            choices: choices.clone(),
        }),
    ) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    };
    assert_eq!(services(&mut app), before + 1);
    assert_eq!(service.title, "Service Plan – November 8, 2026");
    assert_eq!(service.time_zone.as_deref(), Some("America/Jamaica"));
    let stored = match handle(&mut app, Request::Service(ServiceRequest::Get(service.id))) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    };
    assert_eq!(stored, service);
    let lines = describe(&mut app, &service);
    assert_eq!(
        lines,
        [
            "Opening Songs",
            "song Come Thou Almighty King | Key: C",
            "Congregational Song | Not chosen yet in Notion",
            "Praise and Worship",
            "song Indescribable",
            "song I Stand in Awe",
            "song Oceans",
            "Second Scripture Reading",
            "scripture Romans 8:18–21",
        ]
    );
    let _ = indescribable;

    // Importing the same plan again is refused until updating exists (T109).
    let again = draft(&mut app, page());
    assert_eq!(again.existing.as_ref().map(|e| e.id), Some(service.id));
    let refused = app.handle(Request::Plan(PlanRequest::CreateFromPlan {
        draft: again.plan.clone(),
        choices,
    }));
    assert!(
        matches!(refused, Response::Error { ref message } if message.contains("already imported")),
        "{refused:?}"
    );

    // Next week the picks are remembered, also after a restart.
    drop(app);
    let mut app = open(dir.path());
    let next = draft(&mut app, page());
    assert!(matches!(result_of(&next, "s4"), SongResult::Matched { song, .. } if *song == awe));
    assert!(matches!(result_of(&next, "s5"), SongResult::Matched { .. }));
}

fn describe(app: &mut App, service: &Service) -> Vec<String> {
    service
        .ordered()
        .into_iter()
        .map(|(_, item)| {
            let what = match &item.kind {
                ItemKind::Header { title } => title.clone(),
                ItemKind::Song { song, .. } => {
                    match handle(app, Request::Library(LibraryRequest::GetSong(*song))) {
                        Response::Song(s) => format!("song {}", s.title),
                        other => panic!("{other:?}"),
                    }
                }
                ItemKind::Scripture { passage, .. } => format!("scripture {passage}"),
                other => format!("{other:?}"),
            };
            let notes = item
                .notes
                .lines()
                .filter(|l| !l.starts_with("Planned for"))
                .collect::<Vec<_>>()
                .join(" ");
            if notes.is_empty() {
                what
            } else {
                format!("{what} | {notes}")
            }
        })
        .collect()
}

/// The chapel's real 8 November page, read live and matched against a
/// small library, printed as the draft the review screen gets. Needs the
/// network: `cargo test -p lampstand-app --test plan -- --ignored --nocapture`.
#[test]
#[ignore = "reads the live page from notion.so"]
fn drafts_the_live_november_page() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = open(dir.path());
    for title in [
        "Come Thou Almighty King",
        "Indescribable",
        "I Stand in Awe",
        "I Exalt Thee",
        "Holy, Holy, Holy",
    ] {
        put_song(&mut app, title);
    }
    let link = "https://app.notion.com/p/Service-Plan-November-8-2026-Daniel-Fokum-4127e73ae67c813c933eeddf604829ae";
    let (plan, warnings) = tauri::async_runtime::block_on(lampstand_app::plan::read_notion(
        link,
        Some("America/Jamaica".into()),
        Some(-300),
    ))
    .unwrap();
    let mut d = draft(&mut app, plan);
    d.warnings.splice(0..0, warnings);
    assert_eq!(d.plan.title, "Service Plan – November 8, 2026");
    println!("DRAFT {}", serde_json::to_string(&d).unwrap());
}
