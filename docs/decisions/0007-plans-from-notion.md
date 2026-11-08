# 0007. Service plans from Notion

- Status: accepted, revised 2026-11-08 (see "Revision: reading the chapel's pages"); T103 and the page reader are built, T104 to T112 as revised below
- Date: 2026-10-21

## Question

A church plans its Sunday service in Notion, and Breeze holds its people. The projection laptop runs Lampstand, and its songs come from OpenSong. Someone retypes the plan into Lampstand each week. Can Lampstand read the plan from Notion and build the service, matching the songs and scripture against the church's own library, without holding any people and without the church adopting a new planning tool?

## Decision

**Lampstand reads a service plan from a Notion database and turns it into a draft service that the person checks and creates.** It is a connector that produces a [plan](../plan-contract.md), the same source-neutral shape any planning tool can produce. Matching, review and creation are shared with [decision 0003](0003-service-from-list.md) (new service from a list), so the two features resolve songs and passages the same way and differ only in where the entries come from.

Notion stays the church's planning tool. Lampstand never writes to Notion in v1. Planning people and volunteers is out of scope: those columns are never read, and nothing from a person's record reaches Lampstand.

The plan structure the connector reads is a convention (below), which the church adopts in Notion. A "golden example" of it exists as a Notion page, "Service Planning – Golden Example", built to match this document.

## The Notion convention

Three linked databases. The connector finds them from the Services database the person points it at; the other two come from its relations.

**Services**: one row per service.

| Property | Type | Required | Notes |
| --- | --- | --- | --- |
| Title | title | yes | |
| Date | date | no | With a time, it becomes `starts_at`. A date alone reads as the church's usual start time, if set, else midnight and a warning. |
| Time Zone | text | no | IANA name. |
| Status | status | no | `Draft` or `Ready`. Shown, never blocking. |
| Items | relation | yes | Dual relation to Plan Items. |

**Plan Items**: one row per item.

| Property | Type | Required | Notes |
| --- | --- | --- | --- |
| Title | title | yes | |
| Service | relation | yes | To Services. |
| Order | number | yes | Position, 1 first. |
| Type | select | yes | Header, Song, Scripture, Slides, Announcement, Other. |
| Song | relation | songs | To Songs. |
| Key | select | no | Key for this service. |
| Arrangement | text | no | `V1 C V2 C`. |
| Reference | text | scripture | `John 3:16-18`. |
| Translation | select | no | `KJV`, `WEB`, and so on. |
| Notes | text | no | For the operator. |

**Songs**: a catalog for matching only. Lyrics stay in Lampstand.

| Property | Type | Required | Notes |
| --- | --- | --- | --- |
| Title | title | yes | |
| Author | text | no | |
| CCLI Number | number | no | The best matching key. |
| Default Key | select | no | |

Property names are matched ignoring case and surrounding spaces. Extra properties are ignored, which is how a church keeps a people column or a rehearsal-notes column without Lampstand seeing it. A missing required property stops the import with a message naming it, the database it was expected in and the names that were found.

The churches' existing Notion plans will differ. The connector does not try to read free-form pages. Converting a church's plans into this shape is a one-time job, and the golden example is the template.

### Mapping to the plan contract

| Notion | Plan |
| --- | --- |
| Services row id | `source.id` |
| Services row last edited time | `source.edited` |
| Title, Date, Time Zone, Status | `title`, `starts_at`, `time_zone`, `status` |
| Plan Items row id | `entries[].source_id` |
| Order, Type, Title, Notes | `order`, `type`, `title`, `notes` |
| Song relation → Songs row | `entries[].song` `{ title, author, ccli, key }` from Title, Author, CCLI Number, Default Key |
| Key, Arrangement | `key`, `arrangement` |
| Reference, Translation | `reference`, `translation` |

A Song item whose relation is empty uses the item's own Title as the song title.

## Revision: reading the chapel's pages (2026-11-08)

Open question 5 is answered. Mona Heights Chapel's service plans (8 November, 11 October and 23 August 2026) are not the three databases above. Each is an ordinary Notion page: the chairperson's script, one page per Sunday, built from the same template. Asking the planners to retype it into databases each week would move the retyping, not remove it. **Lampstand reads the pages as they are.** The database convention is not built; it stays here as the shape to use if another church wants strict structure.

What the page reader (`connectors::notion::page`) takes from a page:

| On the page | In the plan |
| --- | --- |
| Title "Service Plan – November 8, 2026 – Ada Example" | `title` "Service Plan – November 8, 2026". Segments after the date are a person and are dropped. |
| "Status: Draft" before the order of service | `status` |
| The date in the title, else "Date: 11/08/2026" (US order, as Notion shows it) | with the first section's time and the church's offset, `starts_at` |
| A heading that starts with a time, "10:05 am – Opening Songs" | a `header` entry, notes "Planned for 10:05 am". "Sermon" is `slides`, "Announcements" and "Notices" are `announcement`. |
| "Opening Song: Come Thou Almighty King (C major)", any label with "song" or "hymn" | a `song` entry, `key` "C". A bracket that is not a key stays in the title. |
| An unlabelled list directly under a song, hymn, worship or praise heading | one `song` entry per item |
| A line that starts with a reference, or one labelled "reading", "passage" or "scripture": "Romans 8:18-21 (NKJV) — est. 1 minute", "Communion Passages: Mark 14:12-26; Exodus 12:7-20 (NIV)" | a `scripture` entry per passage; a version given once at the end is for all of them; text after a spaced dash is ignored |
| "TBD" or "TBA" for a song or reading | an `other` entry, notes "Not chosen yet in Notion" |

What it never reads: callouts and quotes (notes for the chairperson, printed verses), anything under an untimed subheading (birthdays, anniversaries, the prayer diary, the weekly schedule with its meeting IDs), labels about people ("Leader:", "Chairperson:", "Speaker:"), and all other free text. Nothing from the page is copied into notes beyond what the reader understood, so the names on the page never reach Lampstand. A test checks this.

The page order is kept even when a time is earlier than the one before it (the 23 August page has 10:15 before 10:05); the review shows a warning. Each entry's `source_id` is the Notion block ID it came from (`<block>#2` for the second passage on a line), so an update can find it again.

**Just the link.** A page shared to the web (Share › Publish) reads from its link alone: no integration, no token, no setup. The chapel's pages are shared this way. `connectors::notion::public` reads them through `POST https://www.notion.so/api/v3/loadPageChunk`, the endpoint Notion's own public page uses. It is not Notion's published API and may change without notice, so it sits in one module, a test reads the live 8 November page (`--ignored`), and the official API stays as the fallback. A private page comes back empty, and Lampstand says how to publish it or connect an integration.

**Private pages.** With an internal integration token saved once in Setup, `NotionClient` reads the same page through the official API: `GET /v1/pages/{id}` and `GET /v1/blocks/{id}/children` (paged, nested blocks followed except inside quotes and child pages), Notion-Version `2025-09-03`. No data-source queries are needed.

Either way, Plan › New service › From Notion asks for the page's link. A page's times carry no time zone, so the review shows when the service starts and a time-zone picker, set to this device's zone, for the person to confirm or change; the page's 10:00 stays 10:00 in the zone picked, with that zone's offset on the service date. Listing a week's pages to pick from needs the parent page and is a follow-up. A callout is a note for the chairperson and is not read, except one that says only "TBD" or "TBA" (the template's 🔶 marker), which is a placeholder.

## How it works

1. **Connect, once per device.** In Setup › Connections the person pastes a Notion integration token and the link to the Services database. The token is stored in the OS keychain as the Google tokens are, never in a synced document, never logged and never shown again after saving. "Test connection" reads the database and reports what it found: the three databases, any missing property, the number of services.
2. **Pick a service.** Plan › New service › From Notion lists the Services rows by date, newest first, marking Draft ones.
3. **Read.** The connector reads the service, its Plan Items and the Songs rows they relate to (see "The Notion API"), and maps them to a plan. A plan is read in one go and held in memory; the fetch does not touch the library.
4. **Resolve, in code.** The shared resolver matches songs and passages as the [contract](../plan-contract.md#matching-rules) describes. The connector never picks a song.
5. **Review.** One row per entry, in order, with its result: matched (and how), likely, choose one, not found. The person can pick a different song, search the library, create a blank song from the Notion catalog row (title, author, CCLI number and key, no lyrics), or leave the placeholder. Nothing is written until the person presses Create.
6. **Create.** One `Repo::put` of the service, written by the person as if they had built it by hand. Each item records the Notion row it came from.
7. **Import again.** Later in the week, the same command on the same Notion service offers "Update from Notion" instead of "Create". See "Updating a service".

### What each entry becomes

As the contract's table, with these Notion details:

- **Key** goes into the item's notes as "Key: A" ahead of the Notion notes, because `ServiceItem` has no key and lyrics are not transposed. If the key differs from the song's own key the note says so ("Key: A (song is in Bb)").
- **Arrangement** tokens are matched to the song's section labels ignoring case ("v1" finds "V1"). Any token with no section keeps the song's own arrangement and warns, because `Song::arranged_by` fails on an unknown label and an unplayable item would fail at the worst moment.
- **Translation** is matched to an installed translation by abbreviation. A missing or uninstalled one falls back to the device's default translation (see "Where the code disagrees") with a warning in the review.
- **Slides, Announcement, Other** become headers with a note, because a Notion row cannot carry a deck. Attaching the deck stays a Plan action (a share link or an upload, CD-9). A "Link" property that holds a Google Slides or OneDrive share link is a natural later column: `connectors::links` can already download those.

## Updating a service

A second import must not undo the operator's work, and must not duplicate items.

- `Service` gets `source: Option<PlanSource>` (kind, id, last edited) and each `ServiceItem` gets `source_id: Option<String>` and `import_hash: Option<String>`, the hash of what the import wrote into it (T107). All are optional and absent in older documents, as `obs_scene` is.
- On update, items are matched by `source_id`. Items without one were added by hand and are left where they are, after the item they followed.
- For a matched item: if its current content hashes to its `import_hash`, nobody has edited it here, and it is replaced by the new import. If it differs, the local edit wins and the review marks it "changed here and in Notion".
- New Notion entries are inserted at the position `Order` gives. Entries gone from Notion are removed if unedited here, and kept and marked otherwise.
- The review shows the update as a list of changes (added, changed, removed, kept) before anything is written.

## The Notion API

To check against Notion's current developer documentation before T104; this was written without access to it.

- **Auth.** An internal integration token sent as a bearer token. The person creates an integration in their own workspace and shares the Services page with it, which is also what limits what Lampstand can see. OAuth with a public integration is not needed and would put Lampstand in the position of holding a client secret.
- **Version.** A `Notion-Version` header on every request. Recent versions split a database into data sources, and queries go to the data source, not the database. The client pins one version and keeps all of it behind `connectors::notion`, so a version change is one file.
- **Reads.** Query a data source with a filter (Plan Items whose Service relation contains the service) and read pages for the related songs. Results are paged; the client follows the cursor.
- **Limits.** Notion allows roughly three requests a second on average. A plan of a dozen items and as many songs is under thirty requests. The client retries on a rate-limit response, honouring `Retry-After`, and stops after a few tries with a readable error.
- **Offline.** Reading needs the network. Once a service is created it is an ordinary Lampstand service and plays offline. Updating needs the network again.
- **No webhooks, no background polling** in v1. The person presses the button.

## Where it lives

- `lampstand-core::plan`: the contract types, parsing and validation (T103).
- `lampstand-connectors::notion` (feature `notion`): the client (T104) and the Notion-to-plan mapping (T105).
- `lampstand-library::plan_match`: the resolver, shared with decision 0003 (T106).
- `lampstand-core::service`: the source and import-hash fields (T107).
- `lampstand-api`: `PlanRequest::{NotionServices, DraftFromNotion, CreateFromPlan, UpdateFromPlan}` and the matching `Response`s (T108, T109).
- App: Setup › Connections (T110) and the review dialog in Plan (T111).

The UI never speaks to Notion. It sends the API messages and shows the answers, as every other screen does.

## Where the code disagrees

- No request sets an item's `notes`, and `InsertItem` takes only a `kind`. `CreateFromPlan` and `UpdateFromPlan` write the whole service at once, which sidesteps it, but Plan will want `ServiceRequest::UpdateItem` anyway (the same gap decision 0003 notes).
- `lampstand-assistant`, which decision 0003 places the resolver in, does not exist. This work puts the resolver in `lampstand-library` instead, where `Library::search_songs` already lives, and 0003 should use it when it is built.
- There is no team or device default translation, and `LookupPassage` takes one per call. The resolver needs one: the first of the installed translations in the order KJV, WEB is the interim, and a setting replaces it.
- `Library` can search lyrics and titles but cannot look a song up by CCLI number. T106 scans `Library::songs()` for it, which is fine for a library of a few thousand songs; an index is a follow-up if it is not.
- `library::search`'s `score` has no meaning on its own. The threshold for "likely" must be set from real data: the church's OpenSong library and a few weeks of its plans (T112).
- The golden example's CCLI numbers were written from memory and are unchecked.

## Risks

- **Wrong matches.** The "likely" rule and the review step defend against them. A match by CCLI number is exact; a match by title is not, and the review shows how each song matched.
- **Songs without CCLI numbers.** OpenSong files often carry no CCLI number, so matching falls to title. That is why the Notion catalog and the person's remembered choices matter, and why week one will ask more than week five.
- **Churches that do not follow the convention.** The importer refuses with a message naming what is missing instead of guessing, which keeps projection safe and may feel strict. The golden example and the "Test connection" report are the mitigation.
- **Notion's API changes.** Isolated in one module, pinned to one version, and covered by fixtures captured from real responses.
- **Tokens.** A token that can read the church's Notion is a secret. Keychain storage, no logging, and a note in Setup to share only the planning page with the integration.
- **Clock and zones.** A service dated 9:00 in Kingston must not read as 9:00 where the laptop sits. `time_zone` is carried into `Service::time_zone` for this reason.

## Open questions

1. Is the Services database the right place to point at, or the page that holds all three? Pointing at the page would make setup one paste, at the cost of finding databases by title.
2. Should an unmatched song offer to create a blank song by default, or stay a placeholder until the person asks? This document says placeholder, as decision 0003 does.
3. Should remembered song choices live on the device (this document) or in a synced document, so every planner's laptop agrees?
4. Does the church want Lampstand to write back to Notion, for example what was actually sung, for CCLI reporting? It is out of v1.
5. Will the church adopt the convention, or does it need a migration from its present pages? Seeing one of its real plans decides this, and the golden example should be compared with it before T105.
