//! A plan: one service's order from a planning tool, with no people in it
//! (T103). The specification is docs/plan-contract.md; these types follow
//! it field for field.
//!
//! A plan names songs and gives references. It never carries lyrics, Bible
//! text, files or anyone's name: the church's library supplies the words,
//! and volunteers stay in the planning tool.

use serde::{Deserialize, Serialize};

/// The contract this module reads and writes.
pub const CONTRACT: &str = "lampstand-plan/1";
const MAJOR: u32 = 1;

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub contract: String,
    pub source: PlanSource,
    pub title: String,
    /// RFC 3339 with an offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starts_at: Option<String>,
    /// IANA name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<PlanStatus>,
    pub entries: Vec<PlanEntry>,
}

/// Where a plan came from, so a second import updates the same service.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub struct PlanSource {
    /// A short word: `notion`, `file`, a tool's name.
    pub kind: String,
    /// The source's own stable ID for this service.
    pub id: String,
    /// The source's last-edited time (RFC 3339), if it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited: Option<String>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanStatus {
    Draft,
    Ready,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanEntry {
    /// The source's stable ID for this item, unique within the plan.
    pub source_id: String,
    /// Entries sort by it; ties break by `source_id`. Gaps are fine.
    pub order: f64,
    #[serde(rename = "type")]
    pub entry_type: EntryType,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub song: Option<SongHint>,
    /// The key for this service ("A", "Bb"). Kept in notes, never transposed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Section labels in the order sung ("V1 C V2 C").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrangement: Option<String>,
    /// Whatever `scripture::parse_reference` accepts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// A translation abbreviation ("KJV", "NKJV").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation: Option<String>,
}

impl PlanEntry {
    /// An entry with only the required fields set.
    pub fn new(
        source_id: impl Into<String>,
        order: f64,
        entry_type: EntryType,
        title: impl Into<String>,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            order,
            entry_type,
            title: title.into(),
            notes: None,
            song: None,
            key: None,
            arrangement: None,
            reference: None,
            translation: None,
        }
    }
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryType {
    Header,
    Song,
    Scripture,
    Slides,
    Announcement,
    Other,
}

/// What the planner knows about a song. Used only for matching.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SongHint {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ccli: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// Not JSON, or a field has the wrong type.
    Malformed(String),
    /// `contract` is missing or not `lampstand-plan/<n>`.
    UnknownContract(String),
    /// A later major version than this build reads.
    NewerContract(String),
    EmptyTitle,
    /// The entry at this position in `entries` has no `order`.
    MissingOrder(usize),
    /// The entry at this position in `entries` has no `source_id`.
    MissingSourceId(usize),
    DuplicateSourceId(String),
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(why) => write!(f, "This plan could not be read: {why}"),
            Self::UnknownContract(c) => write!(f, "This is not a Lampstand plan (contract \"{c}\")."),
            Self::NewerContract(c) => write!(
                f,
                "This plan uses {c}, which is newer than this copy of Lampstand reads. Update Lampstand and try again."
            ),
            Self::EmptyTitle => write!(f, "The plan has no title."),
            Self::MissingOrder(i) => write!(f, "Entry {} has no order, so its place in the service is unknown.", i + 1),
            Self::MissingSourceId(i) => write!(f, "Entry {} has no source ID.", i + 1),
            Self::DuplicateSourceId(id) => write!(f, "Two entries share the source ID \"{id}\"."),
        }
    }
}

impl std::error::Error for PlanError {}

/// Something worth showing in the review that does not stop the import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanWarning {
    /// The entry at this position had a `type` this build does not know; it
    /// reads as `other`.
    UnknownType { index: usize, found: String },
}

impl Plan {
    /// Reads and checks a plan. Unknown fields are ignored; an unknown entry
    /// type reads as `other` with a warning.
    pub fn from_json(json: &str) -> Result<(Plan, Vec<PlanWarning>), PlanError> {
        let raw: raw::Plan =
            serde_json::from_str(json).map_err(|e| PlanError::Malformed(e.to_string()))?;
        check_contract(raw.contract.as_deref().unwrap_or(""))?;

        let mut warnings = Vec::new();
        let mut entries = Vec::with_capacity(raw.entries.len());
        for (index, e) in raw.entries.into_iter().enumerate() {
            let order = e.order.ok_or(PlanError::MissingOrder(index))?;
            let source_id = e
                .source_id
                .filter(|s| !s.is_empty())
                .ok_or(PlanError::MissingSourceId(index))?;
            let entry_type = match e.entry_type.as_str() {
                "header" => EntryType::Header,
                "song" => EntryType::Song,
                "scripture" => EntryType::Scripture,
                "slides" => EntryType::Slides,
                "announcement" => EntryType::Announcement,
                "other" => EntryType::Other,
                found => {
                    warnings.push(PlanWarning::UnknownType {
                        index,
                        found: found.to_string(),
                    });
                    EntryType::Other
                }
            };
            entries.push(PlanEntry {
                source_id,
                order,
                entry_type,
                title: e.title,
                notes: e.notes,
                song: e.song,
                key: e.key,
                arrangement: e.arrangement,
                reference: e.reference,
                translation: e.translation,
            });
        }
        let plan = Plan {
            contract: CONTRACT.to_string(),
            source: raw.source,
            title: raw.title,
            starts_at: raw.starts_at,
            time_zone: raw.time_zone,
            status: raw.status,
            entries,
        };
        plan.validate()?;
        Ok((plan, warnings))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a plan always serialises")
    }

    /// The checks `from_json` makes that apply to a plan built in code.
    pub fn validate(&self) -> Result<(), PlanError> {
        check_contract(&self.contract)?;
        if self.title.trim().is_empty() {
            return Err(PlanError::EmptyTitle);
        }
        let mut seen = std::collections::HashSet::new();
        for (i, e) in self.entries.iter().enumerate() {
            if e.source_id.is_empty() {
                return Err(PlanError::MissingSourceId(i));
            }
            if !seen.insert(e.source_id.as_str()) {
                return Err(PlanError::DuplicateSourceId(e.source_id.clone()));
            }
        }
        Ok(())
    }

    /// The entries by `order`, ties by `source_id`.
    pub fn ordered(&self) -> Vec<&PlanEntry> {
        let mut entries: Vec<&PlanEntry> = self.entries.iter().collect();
        entries.sort_by(|a, b| {
            a.order
                .total_cmp(&b.order)
                .then_with(|| a.source_id.cmp(&b.source_id))
        });
        entries
    }
}

fn check_contract(contract: &str) -> Result<(), PlanError> {
    let major = contract
        .strip_prefix("lampstand-plan/")
        .and_then(|v| v.split('.').next())
        .and_then(|v| v.parse::<u32>().ok())
        .ok_or_else(|| PlanError::UnknownContract(contract.to_string()))?;
    if major > MAJOR {
        return Err(PlanError::NewerContract(contract.to_string()));
    }
    if major == 0 {
        return Err(PlanError::UnknownContract(contract.to_string()));
    }
    Ok(())
}

/// The shape as it arrives, before the checks.
mod raw {
    use serde::Deserialize;

    #[derive(Deserialize)]
    pub struct Plan {
        pub contract: Option<String>,
        pub source: super::PlanSource,
        pub title: String,
        #[serde(default)]
        pub starts_at: Option<String>,
        #[serde(default)]
        pub time_zone: Option<String>,
        #[serde(default)]
        pub status: Option<super::PlanStatus>,
        pub entries: Vec<Entry>,
    }

    #[derive(Deserialize)]
    pub struct Entry {
        #[serde(default)]
        pub source_id: Option<String>,
        #[serde(default)]
        pub order: Option<f64>,
        #[serde(rename = "type", default)]
        pub entry_type: String,
        pub title: String,
        #[serde(default)]
        pub notes: Option<String>,
        #[serde(default)]
        pub song: Option<super::SongHint>,
        #[serde(default)]
        pub key: Option<String>,
        #[serde(default)]
        pub arrangement: Option<String>,
        #[serde(default)]
        pub reference: Option<String>,
        #[serde(default)]
        pub translation: Option<String>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example in docs/plan-contract.md.
    const EXAMPLE: &str = r#"{
      "contract": "lampstand-plan/1",
      "source": { "kind": "notion", "id": "400b84ce-89ae-81ae-89e3-d6caa0de5e10", "edited": "2026-10-11T08:12:00Z" },
      "title": "Sunday Service – October 11, 2026",
      "starts_at": "2026-10-11T09:00:00-05:00",
      "time_zone": "America/Jamaica",
      "status": "ready",
      "entries": [
        { "source_id": "a", "order": 1, "type": "header", "title": "Welcome" },
        { "source_id": "b", "order": 2, "type": "scripture", "title": "Call to Worship", "reference": "Psalm 100:1-5", "translation": "WEB" },
        { "source_id": "c", "order": 4, "type": "song", "title": "Amazing Grace",
          "song": { "title": "Amazing Grace", "author": "John Newton", "ccli": 4755360, "key": "G" },
          "key": "G", "arrangement": "V1 V2 C V3 C" },
        { "source_id": "d", "order": 9, "type": "slides", "title": "Sermon", "notes": "Deck by Thursday" }
      ]
    }"#;

    fn plan_with(entries: &str) -> String {
        format!(
            r#"{{"contract":"lampstand-plan/1","source":{{"kind":"file","id":"x"}},"title":"T","entries":[{entries}]}}"#
        )
    }

    #[test]
    fn example_round_trips() {
        let (plan, warnings) = Plan::from_json(EXAMPLE).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(plan.entries.len(), 4);
        assert_eq!(plan.status, Some(PlanStatus::Ready));
        assert_eq!(plan.entries[2].song.as_ref().unwrap().ccli, Some(4755360));
        let (again, _) = Plan::from_json(&plan.to_json()).unwrap();
        assert_eq!(plan, again);
    }

    #[test]
    fn refuses_a_newer_major_and_a_foreign_contract() {
        let newer = EXAMPLE.replace("lampstand-plan/1", "lampstand-plan/2");
        assert!(matches!(
            Plan::from_json(&newer),
            Err(PlanError::NewerContract(_))
        ));
        let foreign = EXAMPLE.replace("lampstand-plan/1", "something/1");
        assert!(matches!(
            Plan::from_json(&foreign),
            Err(PlanError::UnknownContract(_))
        ));
        let minor = EXAMPLE.replace("lampstand-plan/1", "lampstand-plan/1.3");
        assert!(Plan::from_json(&minor).is_ok());
    }

    #[test]
    fn refuses_an_empty_title() {
        let json = plan_with("").replace(r#""title":"T""#, r#""title":"  ""#);
        assert_eq!(Plan::from_json(&json).unwrap_err(), PlanError::EmptyTitle);
    }

    #[test]
    fn refuses_missing_order_missing_id_and_duplicates() {
        let no_order = plan_with(r#"{"source_id":"a","type":"header","title":"x"}"#);
        assert_eq!(
            Plan::from_json(&no_order).unwrap_err(),
            PlanError::MissingOrder(0)
        );
        let no_id = plan_with(r#"{"order":1,"type":"header","title":"x"}"#);
        assert_eq!(
            Plan::from_json(&no_id).unwrap_err(),
            PlanError::MissingSourceId(0)
        );
        let dup = plan_with(
            r#"{"source_id":"a","order":1,"type":"header","title":"x"},{"source_id":"a","order":2,"type":"header","title":"y"}"#,
        );
        assert_eq!(
            Plan::from_json(&dup).unwrap_err(),
            PlanError::DuplicateSourceId("a".into())
        );
    }

    #[test]
    fn ignores_unknown_fields_and_reads_unknown_types_as_other() {
        let json =
            plan_with(r#"{"source_id":"a","order":1,"type":"video","title":"x","colour":"red"}"#)
                .replace(r#""title":"T""#, r#""title":"T","weather":"sunny""#);
        let (plan, warnings) = Plan::from_json(&json).unwrap();
        assert_eq!(plan.entries[0].entry_type, EntryType::Other);
        assert_eq!(
            warnings,
            vec![PlanWarning::UnknownType {
                index: 0,
                found: "video".into()
            }]
        );
    }

    #[test]
    fn orders_by_order_then_source_id() {
        let json = plan_with(
            r#"{"source_id":"b","order":2,"type":"header","title":"x"},{"source_id":"z","order":1,"type":"header","title":"y"},{"source_id":"a","order":2,"type":"header","title":"w"}"#,
        );
        let (plan, _) = Plan::from_json(&json).unwrap();
        let ids: Vec<&str> = plan
            .ordered()
            .iter()
            .map(|e| e.source_id.as_str())
            .collect();
        assert_eq!(ids, ["z", "a", "b"]);
    }

    #[test]
    fn a_plan_with_no_entries_is_fine() {
        let (plan, _) = Plan::from_json(&plan_with("")).unwrap();
        assert!(plan.entries.is_empty());
    }
}
