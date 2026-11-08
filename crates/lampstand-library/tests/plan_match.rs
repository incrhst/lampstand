//! The plan matcher (T106) and building a service from the review (T108).

use std::collections::HashMap;

use lampstand_core::id::BlobHash;
use lampstand_core::plan::{
    Choice, EntryType, MatchedBy, Outcome, PassageResult, Plan, PlanEntry, PlanSource, SongHint,
    SongResult, CONTRACT,
};
use lampstand_core::scripture::{Licence, Translation, TranslationId};
use lampstand_core::service::ItemKind;
use lampstand_core::song::{Section, Song, SongId};
use lampstand_library::plan_match::{
    build_service, normalise, resolve, song_key, Catalog, Context,
};

/// A typical library: common hymns and worship songs, with the near
/// misses the matcher must not confuse.
mod scores {
    use lampstand_core::song::Song;
    use lampstand_library::search::{perfect_score, SongIndex};

    /// Prints the search scores, as a fraction of a perfect title match,
    /// that LIKELY_AT and LIKELY_MARGIN were set from. Re-derive with
    /// `cargo test -p lampstand-library --test plan_match -- --ignored --nocapture`.
    pub const LIBRARY: &[&str] = &[
        "Amazing Grace",
        "Amazing Grace (My Chains Are Gone)",
        "10,000 Reasons (Bless the Lord)",
        "How Great Thou Art",
        "How Great Is Our God",
        "Come Thou Almighty King",
        "Come Thou Fount of Every Blessing",
        "Indescribable",
        "I Stand in Awe",
        "I Exalt Thee",
        "All Hail the Power of Jesus' Name",
        "Lord, I Lift Your Name on High",
        "Man of Sorrows! What a Name",
        "Man of Sorrows",
        "Great Is Thy Faithfulness",
        "Blessed Assurance",
        "It Is Well with My Soul",
        "Holy, Holy, Holy",
        "To God Be the Glory",
        "Be Thou My Vision",
        "In Christ Alone",
        "The Old Rugged Cross",
        "What a Friend We Have in Jesus",
        "Crown Him with Many Crowns",
        "Here I Am to Worship",
        "Shout to the Lord",
        "Open the Eyes of My Heart",
        "Way Maker",
        "Goodness of God",
        "Build My Life",
        "Cornerstone",
        "Living Hope",
        "Great Are You Lord",
        "Revelation Song",
        "O Come to the Altar",
        "Turn Your Eyes upon Jesus",
        "Because He Lives",
        "Jesus Paid It All",
        "Nothing but the Blood",
        "Holy Spirit",
        "Praise Him! Praise Him!",
        "Rock of Ages",
        "And Can It Be",
        "Thine Be the Glory",
        "Abide with Me",
    ];

    #[test]
    #[ignore = "prints calibration numbers"]
    fn print_scores() {
        let mut index = SongIndex::new();
        for t in LIBRARY {
            index.upsert(&Song::new(*t));
        }
        for q in [
            "10000 reasons",
            "10,000 Reasons",
            "Amazing Grace",
            "My Chains Are Gone",
            "Indescribable",
            "I Stand in Awe of You",
            "I Exalt You (Thou Art High, O Lord, Above All the Earth)",
            "I Exalt You",
            "Lord I Lift Your Name on High",
            "Man of Sorrows, What a Name",
            "All Hail the Power",
            "Come Thou Almighty King",
            "Great is thy faithfullness",
            "How Great",
            "Blessed Assurance Jesus is Mine",
            "Oceans",
            "Holy",
            "Jesus",
            "Thy Faithfulness",
            "Way Maker (Miracle Worker)",
            "Goodnes of God",
            "Living Hope (Phil Wickham)",
            "Crown Him",
            "Abide With Me Fast Falls the Eventide",
        ] {
            let p = perfect_score(q);
            let hits = index.search(q, 3);
            let s: Vec<String> = hits
                .iter()
                .map(|h| format!("{:.2} {}", h.score / p, h.title))
                .collect();
            println!("{q:<60} | {}", s.join(" | "));
        }
    }
}

fn song(title: &str) -> Song {
    Song::new(title)
}

fn library() -> Vec<Song> {
    scores::LIBRARY.iter().map(|t| song(t)).collect()
}

fn translation(abbreviation: &str) -> Translation {
    Translation {
        id: TranslationId::generate(),
        name: abbreviation.into(),
        abbreviation: abbreviation.into(),
        language: "en".into(),
        licence: Licence::PublicDomain,
        text: BlobHash::of(abbreviation.as_bytes()),
    }
}

fn context() -> Context {
    Context {
        translations: vec![translation("KJV"), translation("NKJV")],
        ..Context::default()
    }
}

fn plan(entries: Vec<PlanEntry>) -> Plan {
    Plan {
        contract: CONTRACT.into(),
        source: PlanSource {
            kind: "notion".into(),
            id: "page".into(),
            edited: None,
        },
        title: "Service Plan – November 8, 2026".into(),
        starts_at: Some("2026-11-08T10:00:00-05:00".into()),
        time_zone: Some("America/Jamaica".into()),
        status: None,
        entries,
    }
}

fn song_entry(id: &str, title: &str) -> PlanEntry {
    let mut e = PlanEntry::new(id, 1.0, EntryType::Song, title);
    e.song = Some(SongHint {
        title: title.into(),
        author: None,
        ccli: None,
        key: None,
    });
    e
}

fn song_result(catalog: &Catalog, context: &Context, entry: PlanEntry) -> SongResult {
    let resolved = resolve(&plan(vec![entry]), catalog, context);
    match resolved.entries[0].outcome.clone() {
        Outcome::Song { result, .. } => result,
        other => panic!("not a song: {other:?}"),
    }
}

fn likely_title(r: &SongResult) -> Option<&str> {
    match r {
        SongResult::Likely { title, .. } => Some(title),
        _ => None,
    }
}

#[test]
fn normalises_titles() {
    assert_eq!(normalise("The Old Rugged Cross"), "old rugged cross");
    assert_eq!(
        normalise("Man of Sorrows! What a Name"),
        normalise("man of sorrows, what a name")
    );
    assert_eq!(normalise("Ō Holy Night"), "o holy night");
    assert_eq!(normalise("10,000 Reasons"), "10000 reasons");
}

#[test]
fn ccli_wins_first() {
    let mut a = song("Amazing Grace");
    a.ccli_number = Some(4_755_360);
    let catalog = Catalog::new(vec![a.clone(), song("Something Else")]);
    let mut e = song_entry("e", "A Title That Matches Nothing");
    e.song.as_mut().unwrap().ccli = Some(4_755_360);
    let r = song_result(&catalog, &context(), e);
    assert_eq!(
        r,
        SongResult::Matched {
            song: a.id,
            title: a.title,
            how: MatchedBy::Ccli
        }
    );
}

#[test]
fn two_songs_with_one_ccli_number_ask() {
    let (mut a, mut b) = (song("Amazing Grace"), song("Amazing Grace (alt)"));
    a.ccli_number = Some(1);
    b.ccli_number = Some(1);
    let catalog = Catalog::new(vec![a, b]);
    let mut e = song_entry("e", "Amazing Grace");
    e.song.as_mut().unwrap().ccli = Some(1);
    assert!(
        matches!(song_result(&catalog, &context(), e), SongResult::Choose { candidates } if candidates.len() == 2)
    );
}

#[test]
fn a_remembered_choice_beats_the_title_and_a_deleted_one_is_ignored() {
    let picked = song("Indescribable (Live)");
    let catalog = Catalog::new(vec![song("Indescribable"), picked.clone()]);
    let mut ctx = context();
    ctx.remembered
        .insert(song_key("notion", "Indescribable"), picked.id);
    let r = song_result(&catalog, &ctx, song_entry("e", "Indescribable"));
    assert!(
        matches!(r, SongResult::Matched { how: MatchedBy::Remembered, song, .. } if song == picked.id)
    );

    ctx.remembered
        .insert(song_key("notion", "Indescribable"), SongId::generate());
    let r = song_result(&catalog, &ctx, song_entry("e", "Indescribable"));
    assert!(matches!(
        r,
        SongResult::Matched {
            how: MatchedBy::Title,
            ..
        }
    ));
}

#[test]
fn the_title_matches_ignoring_punctuation_and_the_author_breaks_a_tie() {
    let catalog = Catalog::new(library());
    let r = song_result(
        &catalog,
        &context(),
        song_entry("e", "Man of Sorrows, What a Name"),
    );
    assert!(
        matches!(r, SongResult::Matched { how: MatchedBy::Title, ref title, .. } if title == "Man of Sorrows! What a Name")
    );

    let (mut a, mut b) = (song("Holy Ground"), song("Holy Ground"));
    a.author = Some("Geron Davis".into());
    b.author = Some("Christopher Beatty".into());
    let catalog = Catalog::new(vec![a.clone(), b]);
    let mut e = song_entry("e", "Holy Ground");
    assert!(matches!(
        song_result(&catalog, &context(), e.clone()),
        SongResult::Choose { .. }
    ));
    e.song.as_mut().unwrap().author = Some("Davis".into());
    assert!(
        matches!(song_result(&catalog, &context(), e), SongResult::Matched { song, .. } if song == a.id)
    );
}

#[test]
fn near_misses_are_likely_never_matched() {
    let catalog = Catalog::new(library());
    let ctx = context();
    for (planned, want) in [
        ("10000 reasons", "10,000 Reasons (Bless the Lord)"),
        ("I Stand in Awe of You", "I Stand in Awe"),
        ("Great is thy faithfullness", "Great Is Thy Faithfulness"),
        ("Goodnes of God", "Goodness of God"),
        ("Way Maker (Miracle Worker)", "Way Maker"),
        ("Blessed Assurance Jesus is Mine", "Blessed Assurance"),
        ("Thy Faithfulness", "Great Is Thy Faithfulness"),
    ] {
        let r = song_result(&catalog, &ctx, song_entry("e", planned));
        assert_eq!(likely_title(&r), Some(want), "{planned}: {r:?}");
    }
    // The plain hymn matches itself, not "(My Chains Are Gone)"…
    let r = song_result(&catalog, &ctx, song_entry("e", "Amazing Grace"));
    assert!(matches!(r, SongResult::Matched { ref title, .. } if title == "Amazing Grace"));
    // …and without it, the other is only likely.
    let without: Vec<Song> = library()
        .into_iter()
        .filter(|s| s.title != "Amazing Grace")
        .collect();
    let r = song_result(
        &Catalog::new(without),
        &ctx,
        song_entry("e", "Amazing Grace"),
    );
    assert_eq!(likely_title(&r), Some("Amazing Grace (My Chains Are Gone)"));
}

#[test]
fn vague_or_unknown_titles_ask_or_are_not_found() {
    let catalog = Catalog::new(library());
    let ctx = context();
    for vague in ["Holy", "Jesus", "How Great"] {
        let r = song_result(&catalog, &ctx, song_entry("e", vague));
        assert!(matches!(r, SongResult::Choose { .. }), "{vague}: {r:?}");
    }
    for unknown in [
        "Oceans",
        "I Exalt You (Thou Art High, O Lord, Above All the Earth)",
    ] {
        let r = song_result(&catalog, &ctx, song_entry("e", unknown));
        assert_eq!(r, SongResult::NotFound, "{unknown}");
    }
}

#[test]
fn the_key_goes_in_the_notes() {
    let mut g = song("How Great Thou Art");
    g.key = Some("Bb".into());
    let catalog = Catalog::new(vec![g, song("Indescribable")]);
    let mut a = song_entry("a", "How Great Thou Art");
    a.key = Some("A".into());
    a.notes = Some("Up a step for the congregation".into());
    let mut b = song_entry("b", "Indescribable");
    b.key = Some("C".into());
    let resolved = resolve(&plan(vec![a, b]), &catalog, &context());
    assert_eq!(
        resolved.entries[0].notes,
        "Key: A (song is in Bb)\nUp a step for the congregation"
    );
    assert_eq!(resolved.entries[1].notes, "Key: C");
}

#[test]
fn arrangements_match_labels_and_unknown_ones_warn() {
    let mut s = song("Amazing Grace");
    s.sections = ["V1", "V2", "C"]
        .iter()
        .map(|l| Section {
            label: (*l).into(),
            lines: vec![],
        })
        .collect();
    let catalog = Catalog::new(vec![s]);
    let mut ok = song_entry("a", "Amazing Grace");
    ok.arrangement = Some("v1 c, V2 C".into());
    let mut bad = song_entry("b", "Amazing Grace");
    bad.arrangement = Some("V1 B".into());
    let resolved = resolve(&plan(vec![ok, bad]), &catalog, &context());
    let arrangement = |i: usize| match &resolved.entries[i].outcome {
        Outcome::Song { arrangement, .. } => arrangement.clone(),
        _ => unreachable!(),
    };
    assert_eq!(
        arrangement(0),
        Some(vec!["V1".into(), "C".into(), "V2".into(), "C".into()])
    );
    assert_eq!(arrangement(1), None);
    assert!(resolved.entries[1].warnings[0].contains("\"B\""));
}

fn scripture(id: &str, reference: &str, translation: Option<&str>) -> PlanEntry {
    let mut e = PlanEntry::new(id, 1.0, EntryType::Scripture, reference);
    e.reference = Some(reference.into());
    e.translation = translation.map(String::from);
    e
}

#[test]
fn passages_use_the_named_translation_or_fall_back_with_a_warning() {
    let catalog = Catalog::new(vec![]);
    let ctx = context();
    let resolved = resolve(
        &plan(vec![
            scripture("a", "Romans 8:18–21", Some("NKJV")),
            scripture("b", "Mark 14:12-26", Some("NIV")),
        ]),
        &catalog,
        &ctx,
    );
    let name = |i: usize| match &resolved.entries[i].outcome {
        Outcome::Scripture {
            result: PassageResult::Found {
                translation_name, ..
            },
        } => translation_name.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(name(0), "NKJV");
    assert!(resolved.entries[0].warnings.is_empty());
    assert_eq!(name(1), "KJV");
    assert_eq!(
        resolved.entries[1].warnings,
        ["NIV is not installed, so KJV is used."]
    );

    let none = resolve(
        &plan(vec![scripture("a", "John 3:16", None)]),
        &catalog,
        &Context::default(),
    );
    assert!(
        matches!(&none.entries[0].outcome, Outcome::Placeholder { reason } if reason.contains("no Bible translation"))
    );
}

#[test]
fn an_ambiguous_book_asks_and_nonsense_is_a_placeholder() {
    let catalog = Catalog::new(vec![]);
    let resolved = resolve(
        &plan(vec![
            scripture("a", "J 3:16", None),
            scripture("b", "Hezekiah 4:1", None),
        ]),
        &catalog,
        &context(),
    );
    assert!(matches!(&resolved.entries[0].outcome,
        Outcome::Scripture { result: PassageResult::Choose { candidates, .. } } if candidates.len() > 1));
    assert!(matches!(
        &resolved.entries[1].outcome,
        Outcome::Placeholder { .. }
    ));
}

#[test]
fn builds_the_service_from_the_choices() {
    let indescribable = song("Indescribable");
    let mut lib = library();
    lib.retain(|s| s.title != "Indescribable");
    lib.push(indescribable.clone());
    let catalog = Catalog::new(lib);
    let mut header = PlanEntry::new("h", 1.0, EntryType::Header, "Opening Songs");
    header.notes = Some("Planned for 10:05 am".into());
    let mut tbd = PlanEntry::new("t", 2.0, EntryType::Other, "Congregational Song");
    tbd.notes = Some("Not chosen yet in Notion".into());
    let mut entries = vec![
        header,
        tbd,
        song_entry("s1", "Indescribable"),
        song_entry("s2", "I Stand in Awe of You"),
        song_entry("s3", "Oceans"),
        song_entry("s4", "Thy Faithfulness"),
        scripture("r", "Romans 8:18-21", Some("NKJV")),
        PlanEntry::new("x", 9.0, EntryType::Slides, "Sermon"),
    ];
    for (i, e) in entries.iter_mut().enumerate() {
        e.order = i as f64;
    }
    let p = plan(entries);
    let resolved = resolve(&p, &catalog, &context());

    let mut choices = HashMap::new();
    // s2 is likely and confirmed; s4 is likely and not; s3 becomes a blank song.
    choices.insert("s2".to_string(), Choice::Accept);
    choices.insert("s3".to_string(), Choice::CreateBlank);
    choices.insert("s4".to_string(), Choice::Placeholder);
    let (service, built) = build_service(&resolved, &choices, &catalog);

    let items = service.ordered();
    let kinds: Vec<String> = items
        .iter()
        .map(|(_, i)| match &i.kind {
            ItemKind::Header { title } => format!("header {title}"),
            ItemKind::Song { song, .. } => {
                format!("song {}", catalog_title(&catalog, &built.new_songs, *song))
            }
            ItemKind::Scripture { passage, .. } => format!("scripture {passage}"),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "header Opening Songs",
            "header Congregational Song",
            "song Indescribable",
            // A likely match accepted without a pick is still a placeholder.
            "header ♪ I Stand in Awe of You",
            "song Oceans",
            "header ♪ Thy Faithfulness",
            "scripture Romans 8:18–21",
            "header Sermon",
        ]
    );
    assert_eq!(items[1].1.notes, "Not chosen yet in Notion");
    assert_eq!(built.new_songs.len(), 1);
    assert_eq!(built.new_songs[0].title, "Oceans");
    assert_eq!(
        built.remember.get(&song_key("notion", "Oceans")),
        Some(&built.new_songs[0].id)
    );
    assert_eq!(
        built.remember.get(&song_key("notion", "Indescribable")),
        Some(&indescribable.id)
    );

    // Every item knows where it came from and is unedited.
    assert_eq!(service.source.as_ref().unwrap().id, "page");
    assert_eq!(service.starts_at.unwrap().0, 1_794_150_000_000);
    for (_, item) in &items {
        assert!(item.source_id.is_some());
        assert_eq!(
            item.import_hash.as_deref(),
            Some(item.content_hash().as_str())
        );
    }
}

#[test]
fn picking_a_song_for_a_likely_match_uses_it_and_remembers_it() {
    let catalog = Catalog::new(library());
    let resolved = resolve(
        &plan(vec![song_entry("s", "I Stand in Awe of You")]),
        &catalog,
        &context(),
    );
    let Outcome::Song {
        result: SongResult::Likely { song, .. },
        ..
    } = &resolved.entries[0].outcome
    else {
        panic!()
    };
    let choices = HashMap::from([("s".to_string(), Choice::Song { song: *song })]);
    let (service, built) = build_service(&resolved, &choices, &catalog);
    assert!(matches!(service.ordered()[0].1.kind, ItemKind::Song { song: s, .. } if s == *song));
    assert_eq!(
        built
            .remember
            .get(&song_key("notion", "I Stand in Awe of You")),
        Some(song)
    );
}

fn catalog_title(catalog: &Catalog, new: &[Song], id: SongId) -> String {
    new.iter()
        .find(|s| s.id == id)
        .or_else(|| catalog.song(id))
        .map(|s| s.title.clone())
        .unwrap_or_else(|| format!("missing {id}"))
}
