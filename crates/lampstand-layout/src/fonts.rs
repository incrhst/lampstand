//! The font set shared by layout and render (T20).
//!
//! One [`cosmic_text::FontSystem`] holds the bundled fonts and the system
//! fonts. The renderer (T24) uses the same system through [`Fonts::system`].

use cosmic_text::fontdb::Database;
use cosmic_text::FontSystem;
use lampstand_core::{FontLookup, FontMatch};

/// Family used when nothing else matches.
pub const DEFAULT_FAMILY: &str = "Noto Sans";

/// Requested family (lowercase) → metric-compatible bundled family.
const SUBSTITUTES: &[(&str, &str)] = &[
    ("calibri", "Carlito"),
    ("cambria", "Caladea"),
    ("arial", "Arimo"),
    ("helvetica", "Arimo"),
    ("times new roman", "Tinos"),
    ("courier new", "Cousine"),
];

macro_rules! bundled {
    ($($family:literal),* $(,)?) => {
        &[$(
            bundled!(@file $family, "Regular"),
            bundled!(@file $family, "Bold"),
            bundled!(@file $family, "Italic"),
            bundled!(@file $family, "BoldItalic"),
        )*]
    };
    (@file $family:literal, $style:literal) => {
        (
            concat!($family, "-", $style, ".ttf"),
            include_bytes!(concat!(
                "../../../assets/fonts/", $family, "/", $family, "-", $style, ".ttf"
            )) as &[u8],
        )
    };
}

/// Regular, bold, italic and bold italic of each bundled family, as
/// (file name, bytes).
static BUNDLED: &[(&str, &[u8])] = bundled!(
    "NotoSans",
    "NotoSerif",
    "Carlito",
    "Caladea",
    "Arimo",
    "Tinos",
    "Cousine"
);

/// The bundled font files as (file name such as `NotoSans-BoldItalic.ttf`,
/// bytes), so the OBS browser source can serve the very same fonts.
pub fn bundled_font_files() -> &'static [(&'static str, &'static [u8])] {
    BUNDLED
}

/// Bundled fonts (Noto Sans and Serif, plus the metric-compatible
/// substitutes Carlito, Caladea, Arimo, Tinos and Cousine for Calibri,
/// Cambria, Arial, Times New Roman and Courier New) and the system fonts.
pub struct Fonts {
    system: FontSystem,
}

impl Fonts {
    /// Loads bundled and system fonts. Takes ~100 ms; do it once.
    pub fn load() -> Self {
        Self::build(true)
    }

    /// Loads only the bundled fonts, so results do not depend on the machine.
    pub fn bundled_only() -> Self {
        Self::build(false)
    }

    fn build(system_fonts: bool) -> Self {
        let mut db = Database::new();
        for (_, data) in BUNDLED {
            db.load_font_data(data.to_vec());
        }
        if system_fonts {
            db.load_system_fonts();
        }
        db.set_sans_serif_family(DEFAULT_FAMILY);
        db.set_serif_family("Noto Serif");
        db.set_monospace_family("Cousine");
        let locale = sys_locale();
        Self {
            system: FontSystem::new_with_locale_and_db(locale, db),
        }
    }

    /// The underlying font system, for shaping and for glyphon.
    pub fn system(&mut self) -> &mut FontSystem {
        &mut self.system
    }

    /// Whether a family with this name (case-insensitive) is loaded.
    pub fn has_family(&self, family: &str) -> bool {
        self.system.db().faces().any(|face| {
            face.families
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(family))
        })
    }
}

fn sys_locale() -> String {
    std::env::var("LANG")
        .ok()
        .and_then(|l| l.split('.').next().map(|s| s.replace('_', "-")))
        .filter(|l| !l.is_empty() && l != "C" && l != "POSIX")
        .unwrap_or_else(|| "en-US".to_string())
}

impl FontLookup for Fonts {
    fn lookup(&self, family: &str) -> FontMatch {
        let family = family.trim();
        if self.has_family(family) {
            return FontMatch::Exact;
        }
        let key = family.to_ascii_lowercase();
        match SUBSTITUTES.iter().find(|(from, _)| *from == key) {
            Some((_, to)) => FontMatch::Substitute((*to).to_string()),
            None => FontMatch::Missing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_for_bundled_family() {
        assert_eq!(Fonts::bundled_only().lookup("Carlito"), FontMatch::Exact);
        assert_eq!(Fonts::bundled_only().lookup("noto sans"), FontMatch::Exact);
    }

    #[test]
    fn substitute_for_office_family() {
        let fonts = Fonts::bundled_only();
        let expect = |from: &str, to: &str| {
            assert_eq!(
                fonts.lookup(from),
                FontMatch::Substitute(to.into()),
                "{from}"
            );
        };
        expect("Calibri", "Carlito");
        expect("Cambria", "Caladea");
        expect("Arial", "Arimo");
        expect("Helvetica", "Arimo");
        expect("Times New Roman", "Tinos");
        expect("Courier New", "Cousine");
    }

    #[test]
    fn missing_for_unknown_family() {
        assert_eq!(
            Fonts::bundled_only().lookup("Zapfino Imaginary"),
            FontMatch::Missing
        );
    }

    #[test]
    fn all_styles_of_every_bundled_family_load() {
        let fonts = Fonts::bundled_only();
        assert_eq!(fonts.system.db().faces().count(), BUNDLED.len());
        for family in [
            "Noto Sans",
            "Noto Serif",
            "Carlito",
            "Caladea",
            "Arimo",
            "Tinos",
            "Cousine",
        ] {
            assert!(fonts.has_family(family), "{family}");
        }
    }

    #[test]
    fn load_includes_system_fonts_and_is_fast_enough() {
        // Best of three, so a busy machine does not fail the budget.
        let mut best = std::time::Duration::MAX;
        let mut fonts = None;
        for _ in 0..3 {
            let start = std::time::Instant::now();
            fonts = Some(Fonts::load());
            best = best.min(start.elapsed());
        }
        let bundled_start = std::time::Instant::now();
        let _ = Fonts::bundled_only();
        eprintln!(
            "load: best {best:?}, bundled only {:?}",
            bundled_start.elapsed()
        );
        let mut fonts = fonts.unwrap();
        assert!(fonts.system().db().faces().count() >= BUNDLED.len());
        if !cfg!(debug_assertions) {
            assert!(best.as_millis() < 300, "load took {best:?}");
        }
    }
}
