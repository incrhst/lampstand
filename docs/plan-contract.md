# Plan contract, version 1

A **plan** is one service's order, with no people in it. Anything that plans services can hand one to Lampstand in this shape: the Notion connector ([decision 0007](decisions/0007-plans-from-notion.md)) today, a companion planning tool or a plain file later. Lampstand reads the contract, matches its songs and passages against the church's own library, and writes a normal `Service`. The contract is the only thing the sources and Lampstand share.

The types live in `lampstand-core::plan` (T103), with serde and ts-rs derives like the rest of `core`. This page is the specification the types follow.

## What a plan never holds

- No people, teams, roles or contact details. Volunteers stay in the planning tool (Breeze, Notion, a spreadsheet). Lampstand holds no people.
- No lyrics and no Bible text. A plan names songs and gives references; the church's library supplies the words, so a church's licences and edits stay its own.
- No files. A plan may name a deck ("Sermon") but cannot carry one.

## Shape

```json
{
  "contract": "lampstand-plan/1",
  "source": {
    "kind": "notion",
    "id": "400b84ce-89ae-81ae-89e3-d6caa0de5e10",
    "edited": "2026-10-11T08:12:00Z"
  },
  "title": "Sunday Service – October 11, 2026",
  "starts_at": "2026-10-11T09:00:00-05:00",
  "time_zone": "America/Jamaica",
  "status": "ready",
  "entries": [
    { "source_id": "…-815e-…", "order": 1, "type": "header", "title": "Welcome" },
    {
      "source_id": "…-812b-…", "order": 2, "type": "scripture",
      "title": "Call to Worship", "reference": "Psalm 100:1-5", "translation": "WEB"
    },
    {
      "source_id": "…-812e-…", "order": 4, "type": "song", "title": "Amazing Grace",
      "song": { "title": "Amazing Grace", "author": "John Newton", "ccli": 4755360, "key": "G" },
      "key": "G", "arrangement": "V1 V2 C V3 C"
    },
    {
      "source_id": "…-8133-…", "order": 5, "type": "song", "title": "How Great Thou Art",
      "song": { "title": "How Great Thou Art", "author": "Stuart K. Hine", "ccli": 14181, "key": "Bb" },
      "key": "A", "arrangement": "V1 C V2 C V3 C",
      "notes": "Key up a step from the song default for the congregation"
    },
    {
      "source_id": "…-818a-…", "order": 9, "type": "slides", "title": "Sermon",
      "notes": "Guest speaker deck. Upload link sent Thursday"
    }
  ]
}
```

## Fields

**Plan**

| Field | Required | Meaning |
| --- | --- | --- |
| `contract` | yes | `"lampstand-plan/1"`. A reader that sees a higher major number refuses the plan with a readable reason and does not guess. New optional fields do not change the version. |
| `source` | yes | Where the plan came from, so a second import updates the same service instead of making another. `kind` is a short word (`notion`, `file`, a tool's name). `id` is the source's own stable ID for this service. `edited` is the source's last-edited time, if it has one. |
| `title` | yes | The service's name. Not empty. |
| `starts_at` | no | RFC 3339 with offset. Becomes `Service::starts_at`. |
| `time_zone` | no | IANA name. Becomes `Service::time_zone`. A name Lampstand does not know is dropped with a warning. |
| `status` | no | `draft` or `ready`. Shown in the review; it never blocks an import. |
| `entries` | yes | The items. May be empty. |

**Entry**

| Field | Required | Meaning |
| --- | --- | --- |
| `source_id` | yes | The source's stable ID for this item. Unique within the plan. A second import uses it to find the item it made last time. |
| `order` | yes | A number. Entries sort by it; ties break by `source_id`. Gaps are fine. |
| `type` | yes | `header`, `song`, `scripture`, `slides`, `announcement` or `other`. An unknown type reads as `other` with a warning. |
| `title` | yes | Display name. For a header it is the header text. |
| `notes` | no | For the operator. Never projected. |
| `song` | songs | What the planner knows about the song: `title`, and optionally `author`, `ccli` (number) and `key`. Used only for matching. |
| `key` | no | The key for this service, such as `A` or `Bb`. Lampstand keeps the key in the item's notes for the band; it does not transpose lyrics. |
| `arrangement` | no | Section labels in the order sung, space or comma separated (`V1 C V2 C`). Matched to the song's own labels ignoring case. Blank uses the song's arrangement. |
| `reference` | scripture | Plain reference: `John 3:16-18`, `Ps 23`, `1 Cor 13:4-7`. Whatever `core::scripture::parse_reference` accepts. |
| `translation` | no | Abbreviation (`KJV`, `WEB`). Matched to an installed translation by abbreviation, ignoring case. |

A song entry with no `song` object uses `title` as the song's title. An entry with no `order` is invalid and the whole plan is refused, because guessing an order puts the wrong thing on screen.

## How Lampstand reads each type

| `type` | Becomes | If it cannot |
| --- | --- | --- |
| `header` | `ItemKind::Header { title }` | — |
| `song` | `ItemKind::Song { song, arrangement }` | **Not found** or **choose one**: a placeholder header `♪ <title>` with a note, until the person picks or creates a song. |
| `scripture` | `ItemKind::Scripture { passage, translation }` | Unparseable or ambiguous reference: a placeholder header `📖 <reference>` with the reason in its notes. A missing translation falls back and is warned about. |
| `slides` | A header `<title>` with the note "Deck to attach" plus the entry's notes | Always a placeholder in v1: a plan cannot carry a deck. Attach one in Plan (a share link or an upload). |
| `announcement` | As `slides` | Same. |
| `other` | A header `<title>` with the entry's notes | — |

Nothing is dropped silently. Every entry becomes an item, and every placeholder says why in its notes and in the review. The review step shows each entry's result before anything is written.

## Matching rules

Songs, first rule that gives exactly one candidate wins:

1. `song.ccli` equals a library song's CCLI number.
2. The title matches exactly after normalising (case, punctuation, accents and a leading "the" ignored). If several songs match, `song.author` breaks the tie.
3. Library search (`Library::search_songs`) returns one clear best hit above the confidence threshold. This is **likely**, not exact: the person confirms it in the review.

Several candidates is **choose one**. None is **not found**. A person's choice is remembered on this device, keyed by the source's song ID where there is one, so next week's plan does not ask again.

Passages go through `parse_reference`; an ambiguous book ("J 3") is **choose one** with its candidates.

## Companion tools

A planning tool that wants to work with Lampstand only has to produce this JSON, as a file ("Import plan…") or at a URL Lampstand is told to poll. It does not need Lampstand's IDs, lyrics or licences, and it can leave out anything optional. The Notion connector is one such producer, and its mapping is in [decision 0007](decisions/0007-plans-from-notion.md).

## Versioning

- `lampstand-plan/1` fields are never removed or retyped.
- New optional fields may appear; readers ignore fields they do not know.
- A change that breaks a reader makes `lampstand-plan/2`. Lampstand reads every version it has shipped.
