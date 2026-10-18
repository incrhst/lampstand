//! The 66 books of the Protestant canon, their identifiers in each import
//! format, and lookup by the names and abbreviations operators type.

use std::fmt;

struct BookInfo {
    name: &'static str,
    osis: &'static str,
    usfm: &'static str,
    chapters: u16,
    /// Extra abbreviations beyond prefixes of the name, already normalised
    /// (lowercase, no spaces or dots, leading digit for numbered books).
    aliases: &'static [&'static str],
}

const fn b(
    name: &'static str,
    osis: &'static str,
    usfm: &'static str,
    chapters: u16,
    aliases: &'static [&'static str],
) -> BookInfo {
    BookInfo {
        name,
        osis,
        usfm,
        chapters,
        aliases,
    }
}

const BOOKS: [BookInfo; 66] = [
    b("Genesis", "Gen", "GEN", 50, &["gn"]),
    b("Exodus", "Exod", "EXO", 40, &["exo", "exod", "ex"]),
    b("Leviticus", "Lev", "LEV", 27, &["lv"]),
    b("Numbers", "Num", "NUM", 36, &["nm", "nb"]),
    b("Deuteronomy", "Deut", "DEU", 34, &["dt"]),
    b("Joshua", "Josh", "JOS", 24, &["jos", "jsh"]),
    b("Judges", "Judg", "JDG", 21, &["jdg", "jg", "jdgs"]),
    b("Ruth", "Ruth", "RUT", 4, &["rth", "rt"]),
    b("1 Samuel", "1Sam", "1SA", 31, &["1sm", "1s"]),
    b("2 Samuel", "2Sam", "2SA", 24, &["2sm", "2s"]),
    b("1 Kings", "1Kgs", "1KI", 22, &["1kgs", "1kg", "1k"]),
    b("2 Kings", "2Kgs", "2KI", 25, &["2kgs", "2kg", "2k"]),
    b("1 Chronicles", "1Chr", "1CH", 29, &[]),
    b("2 Chronicles", "2Chr", "2CH", 36, &[]),
    b("Ezra", "Ezra", "EZR", 10, &["ezr"]),
    b("Nehemiah", "Neh", "NEH", 13, &[]),
    b("Esther", "Esth", "EST", 10, &["es"]),
    b("Job", "Job", "JOB", 42, &["jb"]),
    b(
        "Psalms",
        "Ps",
        "PSA",
        150,
        &["psalm", "ps", "psa", "pss", "psm"],
    ),
    b("Proverbs", "Prov", "PRO", 31, &["prv"]),
    b("Ecclesiastes", "Eccl", "ECC", 12, &["qoh", "qoheleth"]),
    b(
        "Song of Solomon",
        "Song",
        "SNG",
        8,
        &["songofsongs", "sos", "ss", "sng", "canticles", "cant"],
    ),
    b("Isaiah", "Isa", "ISA", 66, &[]),
    b("Jeremiah", "Jer", "JER", 52, &["jr"]),
    b("Lamentations", "Lam", "LAM", 5, &[]),
    b("Ezekiel", "Ezek", "EZK", 48, &["eze", "ezk"]),
    b("Daniel", "Dan", "DAN", 12, &["dn"]),
    b("Hosea", "Hos", "HOS", 14, &[]),
    b("Joel", "Joel", "JOL", 3, &["jl"]),
    b("Amos", "Amos", "AMO", 9, &[]),
    b("Obadiah", "Obad", "OBA", 1, &[]),
    b("Jonah", "Jonah", "JON", 4, &["jnh"]),
    b("Micah", "Mic", "MIC", 7, &[]),
    b("Nahum", "Nah", "NAM", 3, &["nam"]),
    b("Habakkuk", "Hab", "HAB", 3, &["hb"]),
    b("Zephaniah", "Zeph", "ZEP", 3, &["zp"]),
    b("Haggai", "Hag", "HAG", 2, &["hg"]),
    b("Zechariah", "Zech", "ZEC", 14, &["zc"]),
    b("Malachi", "Mal", "MAL", 4, &["ml"]),
    b("Matthew", "Matt", "MAT", 28, &["mt"]),
    b("Mark", "Mark", "MRK", 16, &["mk", "mrk", "mr"]),
    b("Luke", "Luke", "LUK", 24, &["lk"]),
    b("John", "John", "JHN", 21, &["jn", "jhn"]),
    b("Acts", "Acts", "ACT", 28, &["ac"]),
    b("Romans", "Rom", "ROM", 16, &["rm"]),
    b("1 Corinthians", "1Cor", "1CO", 16, &[]),
    b("2 Corinthians", "2Cor", "2CO", 13, &[]),
    b("Galatians", "Gal", "GAL", 6, &[]),
    b("Ephesians", "Eph", "EPH", 6, &[]),
    b("Philippians", "Phil", "PHP", 4, &["phil", "php", "pp"]),
    b("Colossians", "Col", "COL", 4, &[]),
    b("1 Thessalonians", "1Thess", "1TH", 5, &[]),
    b("2 Thessalonians", "2Thess", "2TH", 3, &[]),
    b("1 Timothy", "1Tim", "1TI", 6, &[]),
    b("2 Timothy", "2Tim", "2TI", 4, &[]),
    b("Titus", "Titus", "TIT", 3, &[]),
    b("Philemon", "Phlm", "PHM", 1, &["phlm", "phm", "philem"]),
    b("Hebrews", "Heb", "HEB", 13, &[]),
    b("James", "Jas", "JAS", 5, &["jm"]),
    b("1 Peter", "1Pet", "1PE", 5, &["1pt"]),
    b("2 Peter", "2Pet", "2PE", 3, &["2pt"]),
    b("1 John", "1John", "1JN", 5, &["1jn", "1jhn"]),
    b("2 John", "2John", "2JN", 1, &["2jn", "2jhn"]),
    b("3 John", "3John", "3JN", 1, &["3jn", "3jhn"]),
    b("Jude", "Jude", "JUD", 1, &["jud"]),
    b(
        "Revelation",
        "Rev",
        "REV",
        22,
        &["revelations", "rv", "apocalypse"],
    ),
];

/// A book of the Bible. Ordered canonically, Genesis first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Book(u8);

/// Why a typed book name did not resolve to one book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookError {
    Unknown {
        input: String,
        suggestions: Vec<Book>,
    },
    Ambiguous {
        input: String,
        candidates: Vec<Book>,
    },
}

impl fmt::Display for BookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BookError::Unknown { input, suggestions } if suggestions.is_empty() => {
                write!(f, "There is no book called \u{201c}{input}\u{201d}")
            }
            BookError::Unknown { input, suggestions } => write!(
                f,
                "There is no book called \u{201c}{input}\u{201d}. Did you mean {}?",
                or_list(suggestions)
            ),
            BookError::Ambiguous { input, candidates } => write!(
                f,
                "\u{201c}{input}\u{201d} could be {}. Type a little more of the name",
                or_list(candidates)
            ),
        }
    }
}

impl std::error::Error for BookError {}

fn or_list(books: &[Book]) -> String {
    let names: Vec<&str> = books.iter().map(|b| b.name()).collect();
    match names.split_last() {
        None => String::new(),
        Some((last, [])) => last.to_string(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    }
}

impl Book {
    pub const GENESIS: Book = Book(0);
    pub const PSALMS: Book = Book(18);
    pub const JOHN: Book = Book(42);
    pub const REVELATION: Book = Book(65);

    fn info(self) -> &'static BookInfo {
        &BOOKS[self.0 as usize]
    }

    /// All 66 books in canonical order.
    pub fn all() -> impl Iterator<Item = Book> {
        (0..BOOKS.len() as u8).map(Book)
    }

    /// Book by canonical position, 1 = Genesis, 66 = Revelation (Zefania's
    /// `bnumber`).
    pub fn from_number(n: u32) -> Option<Book> {
        (1..=66).contains(&n).then(|| Book(n as u8 - 1))
    }

    pub fn from_osis(id: &str) -> Option<Book> {
        Book::all().find(|b| b.osis().eq_ignore_ascii_case(id))
    }

    pub fn from_usfm(code: &str) -> Option<Book> {
        Book::all().find(|b| b.usfm().eq_ignore_ascii_case(code))
    }

    /// Canonical position, 1 = Genesis.
    pub fn number(self) -> u32 {
        self.0 as u32 + 1
    }

    /// English name, e.g. "1 Corinthians".
    pub fn name(self) -> &'static str {
        self.info().name
    }

    pub fn osis(self) -> &'static str {
        self.info().osis
    }

    pub fn usfm(self) -> &'static str {
        self.info().usfm
    }

    /// Number of chapters in the standard English versification.
    pub fn chapters(self) -> u16 {
        self.info().chapters
    }

    /// Books with one chapter, where "Jude 3" means verse 3.
    pub fn is_single_chapter(self) -> bool {
        self.chapters() == 1
    }

    /// Resolve a typed name or abbreviation: "Jn", "1 Cor", "I Corinthians",
    /// "Song of Songs", "ps". Matching ignores case, spaces and dots. An exact
    /// name, OSIS/USFM id or known abbreviation wins; otherwise the input
    /// must be the start of exactly one book's name.
    pub fn parse(input: &str) -> Result<Book, BookError> {
        let key = normalise(input);
        if key.is_empty() {
            return Err(BookError::Unknown {
                input: input.trim().to_string(),
                suggestions: vec![],
            });
        }
        let exact = Book::all().find(|b| {
            let info = b.info();
            normalise(info.name) == key
                || info.osis.eq_ignore_ascii_case(&key)
                || info.usfm.eq_ignore_ascii_case(&key)
                || info.aliases.contains(&key.as_str())
        });
        if let Some(book) = exact {
            return Ok(book);
        }
        let candidates: Vec<Book> = Book::all()
            .filter(|b| normalise(b.name()).starts_with(&key))
            .collect();
        match candidates.len() {
            1 => Ok(candidates[0]),
            0 => Err(BookError::Unknown {
                input: input.trim().to_string(),
                suggestions: suggest(&key),
            }),
            _ => Err(BookError::Ambiguous {
                input: input.trim().to_string(),
                candidates,
            }),
        }
    }
}

impl fmt::Display for Book {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Lowercase, drop spaces and dots, and turn a leading ordinal ("I", "II",
/// "1st", "Second") into a digit, so "II Cor." and "2cor" compare equal.
pub(crate) fn normalise(input: &str) -> String {
    let lower = input.trim().to_lowercase();
    let mut words = lower
        .split(|c: char| c.is_whitespace() || c == '.')
        .filter(|w| !w.is_empty())
        .peekable();
    let mut out = String::new();
    if let Some(first) = words.peek().copied() {
        let ordinal = match first {
            "i" | "1st" | "first" => Some('1'),
            "ii" | "2nd" | "second" => Some('2'),
            "iii" | "3rd" | "third" => Some('3'),
            _ => None,
        };
        // A lone "I"/"II" only counts as an ordinal when a name follows.
        if let Some(digit) = ordinal {
            words.next();
            if words.peek().is_some() {
                out.push(digit);
            } else {
                out.push_str(first);
            }
        }
    }
    for w in words {
        out.push_str(w);
    }
    out
}

/// Books whose name or abbreviations are within a small edit distance of the
/// typed key, closest first.
fn suggest(key: &str) -> Vec<Book> {
    let mut scored: Vec<(usize, Book)> = Book::all()
        .filter_map(|b| {
            let name = normalise(b.name());
            let prefix: String = name.chars().take(key.chars().count()).collect();
            let best = [edit_distance(key, &name), edit_distance(key, &prefix)]
                .into_iter()
                .chain(b.info().aliases.iter().map(|a| edit_distance(key, a)))
                .min()
                .unwrap_or(usize::MAX);
            let limit = if key.chars().count() <= 3 { 1 } else { 2 };
            (best <= limit).then_some((best, b))
        })
        .collect();
    scored.sort();
    // Only the closest matches: "Jhon" suggests John, not Job as well.
    let best = scored.first().map(|(d, _)| *d);
    scored
        .into_iter()
        .filter(|(d, _)| Some(*d) == best)
        .map(|(_, b)| b)
        .take(3)
        .collect()
}

/// Levenshtein distance counting an adjacent swap as one edit, which is how
/// most typos ("Jhon") happen.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> &'static str {
        Book::parse(s).unwrap_or_else(|e| panic!("{s}: {e}")).name()
    }

    #[test]
    fn common_abbreviations() {
        let cases = [
            ("Gen", "Genesis"),
            ("Ex", "Exodus"),
            ("Ps", "Psalms"),
            ("Psalm", "Psalms"),
            ("Jn", "John"),
            ("Jhn", "John"),
            ("1 Cor", "1 Corinthians"),
            ("1Co", "1 Corinthians"),
            ("I Cor.", "1 Corinthians"),
            ("II Kings", "2 Kings"),
            ("2nd Tim", "2 Timothy"),
            ("First John", "1 John"),
            ("Song of Songs", "Song of Solomon"),
            ("Phil", "Philippians"),
            ("Phlm", "Philemon"),
            ("Mt", "Matthew"),
            ("Mk", "Mark"),
            ("Lk", "Luke"),
            ("Rev", "Revelation"),
            ("Isa", "Isaiah"),
            ("Jas", "James"),
            ("Jude", "Jude"),
            ("Judg", "Judges"),
            ("Eccl", "Ecclesiastes"),
            ("Ezek", "Ezekiel"),
            ("ROM", "Romans"),
        ];
        for (input, name) in cases {
            assert_eq!(parse(input), name, "{input}");
        }
    }

    #[test]
    fn every_alias_and_name_resolves_to_its_own_book() {
        for book in Book::all() {
            assert_eq!(Book::parse(book.name()), Ok(book));
            assert_eq!(Book::parse(book.osis()), Ok(book));
            assert_eq!(Book::parse(book.usfm()), Ok(book));
            for alias in book.info().aliases {
                assert_eq!(Book::parse(alias), Ok(book), "{alias}");
            }
        }
    }

    #[test]
    fn ambiguous_prefix_lists_candidates() {
        let err = Book::parse("Jo").unwrap_err();
        assert_eq!(
            err.to_string(),
            "\u{201c}Jo\u{201d} could be Joshua, Job, Joel, Jonah or John. Type a little more of the name"
        );
    }

    #[test]
    fn typo_suggests_closest_book() {
        match Book::parse("Jhon").unwrap_err() {
            BookError::Unknown { suggestions, .. } => {
                assert_eq!(suggestions.first(), Some(&Book::JOHN))
            }
            e => panic!("{e}"),
        }
        match Book::parse("Genisis").unwrap_err() {
            BookError::Unknown { suggestions, .. } => assert_eq!(suggestions, vec![Book::GENESIS]),
            e => panic!("{e}"),
        }
    }

    #[test]
    fn ids_and_numbers() {
        assert_eq!(Book::from_number(1), Some(Book::GENESIS));
        assert_eq!(Book::from_number(66), Some(Book::REVELATION));
        assert_eq!(Book::from_number(67), None);
        assert_eq!(Book::from_osis("Ps"), Some(Book::PSALMS));
        assert_eq!(Book::from_usfm("JHN"), Some(Book::JOHN));
        assert_eq!(Book::all().map(|b| b.chapters() as u32).sum::<u32>(), 1189);
    }
}
