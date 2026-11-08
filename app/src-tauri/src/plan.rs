//! Services from a planning tool (decision 0007, T108): `PlanRequest`.
//!
//! The shell reads a Notion page ([`read_notion`], async, off the `App`
//! lock) and hands the plan to the app as `DraftFromPlan`. The app matches
//! it against the library and answers with the draft ([`draft`]); nothing
//! is written until the person presses Create ([`create`]).
//!
//! The songs the person picks are remembered on this device
//! (`plan-choices.json`), so next week's plan does not ask again.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use lampstand_api::{EntryChoice, ExistingService, PlanDraft};
use lampstand_connectors::notion::page::{page_to_plan, ReadOptions, ReadWarning};
use lampstand_connectors::notion::public::PublicReader;
use lampstand_core::plan::{Plan, ResolvedPlan};
use lampstand_core::service::Service;
use lampstand_core::song::SongId;
use lampstand_library::plan_match::{build_service, resolve, Catalog, Context};
use lampstand_library::Library;
use lampstand_sync::write_atomic;

/// The songs picked in earlier reviews: `plan_match::song_key` → song.
pub struct PlanChoices {
    path: PathBuf,
    map: HashMap<String, SongId>,
}

impl PlanChoices {
    /// Reads `path`, or nothing if it is missing or unreadable: it holds
    /// only conveniences that the next review rebuilds.
    pub fn load(path: PathBuf) -> Self {
        let map = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self { path, map }
    }

    pub fn get(&self) -> &HashMap<String, SongId> {
        &self.map
    }

    pub fn remember(&mut self, choices: HashMap<String, SongId>) -> io::Result<()> {
        if choices.is_empty() {
            return Ok(());
        }
        self.map.extend(choices);
        let json = serde_json::to_vec_pretty(&self.map).map_err(io::Error::other)?;
        write_atomic(&self.path, &json)
    }
}

/// Reads a Notion page shared to the web into a plan, with what the reader
/// noticed as plain sentences.
pub async fn read_notion(
    link: &str,
    time_zone: Option<String>,
    utc_offset_minutes: Option<i32>,
) -> Result<(Plan, Vec<String>), String> {
    let (meta, blocks) = PublicReader::default()
        .read_page(link)
        .await
        .map_err(|e| e.to_string())?;
    let options = ReadOptions {
        time_zone,
        utc_offset_minutes,
    };
    let (plan, warnings) = page_to_plan(&meta, &blocks, &options).map_err(|e| e.to_string())?;
    Ok((plan, warnings.iter().map(warning_text).collect()))
}

fn warning_text(w: &ReadWarning) -> String {
    match w {
        ReadWarning::NoDate => "The page gives no date, so the service has no start time.".into(),
        ReadWarning::NoTimeZone => {
            "This device's time zone is not known, so the service has no start time.".into()
        }
        ReadWarning::TimeOutOfOrder { heading } => {
            format!(
                "\"{heading}\" is earlier than the section before it. The page's order is kept."
            )
        }
        ReadWarning::UnreadReference { text } => {
            format!("\"{text}\" looks like a reading but could not be read.")
        }
    }
}

/// Matches a plan against the library. Writes nothing.
pub fn draft(library: &Library, plan: &Plan, choices: &PlanChoices) -> Result<PlanDraft, String> {
    plan.validate().map_err(|e| e.to_string())?;
    let catalog = Catalog::new(library.songs().map_err(|e| e.to_string())?);
    let context = Context {
        translations: library.translations().map_err(|e| e.to_string())?,
        default_translation: None,
        remembered: choices.get().clone(),
    };
    let resolved = resolve(plan, &catalog, &context);
    let existing = existing(library, &resolved)?;
    Ok(PlanDraft {
        plan: resolved,
        warnings: Vec::new(),
        existing,
    })
}

/// Writes the service the reviewed draft describes.
pub fn create(
    library: &Library,
    draft: &ResolvedPlan,
    choices: &[EntryChoice],
    remembered: &mut PlanChoices,
) -> Result<Service, String> {
    if let Some(service) = existing(library, draft)? {
        return Err(format!(
            "This plan was already imported as \"{}\". Delete that service to import it again; updating it from the plan comes later.",
            service.title
        ));
    }
    let catalog = Catalog::new(library.songs().map_err(|e| e.to_string())?);
    let by_entry = choices
        .iter()
        .map(|c| (c.source_id.clone(), c.choice.clone()))
        .collect();
    let (service, built) = build_service(draft, &by_entry, &catalog);
    for song in &built.new_songs {
        library.put_song(song).map_err(|e| e.to_string())?;
    }
    library.put_service(&service).map_err(|e| e.to_string())?;
    if let Err(e) = remembered.remember(built.remember) {
        // The service is written; forgetting a choice only means asking again.
        eprintln!("lampstand: could not remember the plan's song choices: {e}");
    }
    Ok(service)
}

/// The service an earlier import of this plan made.
fn existing(library: &Library, plan: &ResolvedPlan) -> Result<Option<ExistingService>, String> {
    let services = library.services().map_err(|e| e.to_string())?;
    Ok(services
        .into_iter()
        .find(|s| {
            s.source
                .as_ref()
                .is_some_and(|src| src.kind == plan.source.kind && src.id == plan.source.id)
        })
        .map(|s| ExistingService {
            id: s.id,
            title: s.title,
        }))
}
