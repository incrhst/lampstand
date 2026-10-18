//! Search over the imported fixture library.

use std::path::Path;

use lampstand_songs::import_dir;
use lampstand_songs::search::SongIndex;

fn library() -> SongIndex<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut index = SongIndex::new();
    for (_, result) in import_dir(&dir).unwrap() {
        if let Ok(imported) = result {
            for song in imported.songs {
                index.insert(song.title.clone(), &song);
            }
        }
    }
    index
}

fn top(index: &SongIndex<String>, query: &str) -> Option<String> {
    index.search(query, 5).into_iter().next().map(|h| h.key)
}

#[test]
fn finds_by_title_lyrics_and_author() {
    let index = library();
    assert_eq!(index.len(), 4);
    assert_eq!(top(&index, "amazing grace").as_deref(), Some("Amazing Grace"));
    assert_eq!(top(&index, "glassy sea").as_deref(), Some("Holy, Holy, Holy"));
    assert_eq!(top(&index, "fanny crosby").as_deref(), Some("Blessed Assurance"));
    assert_eq!(top(&index, "lord god almighty").as_deref(), Some("Holy, Holy, Holy"));
}

#[test]
fn tolerates_typos() {
    let index = library();
    assert_eq!(top(&index, "amazng graec").as_deref(), Some("Amazing Grace"));
    assert_eq!(top(&index, "blesed asurance").as_deref(), Some("Blessed Assurance"));
    assert_eq!(top(&index, "foretste of glory").as_deref(), Some("Blessed Assurance"));
}

#[test]
fn matches_prefix_while_typing() {
    let index = library();
    assert_eq!(top(&index, "silent ni").as_deref(), Some("Silent Night"));
    // Once the word is finished, a bare prefix no longer matches.
    assert!(index.search("calm bri ", 5).iter().all(|h| h.key == "Silent Night"));
    assert!(index.search("bri ", 5).is_empty());
    assert!(index.search("xyzzy", 5).is_empty());
}

#[test]
fn ignores_chords_case_and_apostrophes() {
    let index = library();
    // "[D]morn[E]ing" in the source must be indexed as "morning".
    assert_eq!(top(&index, "MORNING").as_deref(), Some("Holy, Holy, Holy"));
    assert_eq!(top(&index, "twas grace").as_deref(), Some("Amazing Grace"));
    assert!(index.search("Bm", 5).is_empty());
}

#[test]
fn finds_by_ccli_number() {
    assert_eq!(top(&library(), "22025").as_deref(), Some("Amazing Grace"));
}

#[test]
fn songs_matching_every_word_rank_first() {
    let index = library();
    // "holy" is all over one song; "night" only in the other. The song with
    // both words wins.
    assert_eq!(top(&index, "holy night").as_deref(), Some("Silent Night"));
}

#[test]
fn replace_and_remove() {
    let mut index = library();
    assert!(index.remove(&"Silent Night".to_string()));
    assert!(!index.remove(&"Silent Night".to_string()));
    assert_eq!(index.len(), 3);
    assert!(index.search("silent", 5).is_empty());
}
