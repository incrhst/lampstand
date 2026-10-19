# 0003. New service from a list, with a small local Gemma model

- Status: proposed (not implemented; no ticket yet)
- Date: 2026-10-18

## Question

The worship leader sends the week's songs and readings as a message:

> Sunday 26th – Come Thou Fount, 10000 reasons (key of G), Ps 23,
> sermon John 10:1-18, then In Christ Alone to close

Today someone builds that service by hand, one search at a time. Can
Lampstand turn such a list into a service in one step, using an AI model,
without sending the church's library anywhere and without the model ever
touching what is on screen?

## Decision

**"New service from a list"** in Plan. The person pastes or types a list,
the app turns it into a draft service, the person checks each line's match
and presses **Create**. The parsing is done on this computer, by a small
Gemma model where one is available and by a plain line parser where not.
The model only reads text and returns structure; Lampstand's own code
finds the songs and passages and writes the service. Nothing leaves the
machine.

No MCP, no cloud model, no relay involvement.

## How it works

1. **Input.** A text box, a date (the coming Sunday by default, from the
   scheduling change proposed separately) and a title (defaulting to
   "Sunday 26 October").
2. **Extract.** The text becomes an ordered list of entries:

   ```json
   [{ "kind": "song", "title": "10000 reasons", "key": "G" },
    { "kind": "scripture", "reference": "Ps 23" },
    { "kind": "header", "title": "Sermon" },
    { "kind": "scripture", "reference": "John 10:1-18" },
    { "kind": "song", "title": "In Christ Alone" }]
   ```

   With a model: one request, its output constrained to this JSON schema by
   the runtime, so it always parses. The model's whole job is splitting
   messy text ("then … to close", commas, dashes, bullet characters, a
   date mixed in) into entries and deciding which are songs, readings or
   section headings. That is extraction, which small models do well.
   Without a model: one entry per line or comma, a line that
   `parse_reference` accepts is scripture, anything else is a song title.
   Clean lists work either way.
3. **Resolve, in code.** Songs go through the library's search
   (`library::search`, typo-tolerant, title matches first): one strong hit
   is matched; close runners-up are offered as a choice; no hit is "not in
   the library". References go through `core::scripture::parse_reference`
   and the team's default translation; an ambiguous book ("J 3") is offered
   as a choice. A song key, if given, goes into the item's notes for the
   band. The model never picks the song or the passage; it cannot invent an
   ID, because it never sees any.
4. **Review.** One row per entry, in order, each marked matched, choose
   one, or not found. "Not found" rows become a header item named after the
   text with a note, so nothing is silently dropped and the gap is visible
   in Plan; the person can also search and pick right there.
5. **Create.** One `Repo::put` of the new service, written by the person,
   as if they had built it by hand. Undo is deleting the service.

The person starts it and reviews it straight away, so v1 needs no
assistant identity, no new role and no suggestion documents. Those belong
to a later, open-ended assistant that drafts while nobody is watching (see
"Later").

## The model, and "where available"

**Model.** A small instruction-tuned Gemma, quantised to about 4 bits. The
1B class is enough to try; the 4B class (about 3 GB of memory) is the
safer default for messy text. The model name is a setting, so a newer small
Gemma can be swapped in without a code change. An evaluation set of thirty
real-looking lists (WhatsApp messages, emails, numbered lists, mixed
languages from the song databases) with expected entries gates the default
and catches regressions.

**Runtime.** Lampstand talks over loopback HTTP to a local runtime the
machine already has: Ollama, LM Studio or a llama.cpp server. All serve an
OpenAI-compatible chat endpoint and constrain output to a JSON schema. The
app probes their usual loopback ports for a runtime with a Gemma model.
If one answers, the dialog says "Using Gemma on this computer"; if not, the
line parser is used and the dialog says that messy lists work better with
a local model, with a link to the setup steps. The feature is always there;
the model makes it better.

**Not embedding the model, for now.** Bundling llama.cpp and downloading
the weights on opt-in (as the song databases are) would be one install for
volunteers, but adds a native build dependency on every CI platform, a 1
to 3 GB download for the app to manage, and the model's licence arriving
through our installer. Revisit if runtime setup proves too hard.

**Licence.** Gemma weights come under Google's Gemma Terms of Use and
Prohibited Use Policy, not an open-source licence. Lampstand (GPL-3.0)
neither bundles nor downloads them; the user gets them through their
runtime and accepts those terms there.

**Live safety.** A model generating on the presenting machine competes for
the GPU and memory the 50 ms click-to-frame budget needs. While
`LiveEngine::is_idle()` is false, the dialog uses the line parser only and
says why. One request per list, typically a few seconds, keeps the cost
small otherwise.

## Where it lives

- A small `lampstand-assistant` crate (services layer, beside `library`):
  the extraction schema and prompt, the runtime probe and HTTP client
  (loopback only), the line parser, and the resolver. Extraction and
  resolution are pure functions over text and a `Library`, unit-tested with
  the evaluation set and a fixture library; only the HTTP call is not.
- API: `ServiceRequest::DraftFromList { text, starts_at }` answers
  `Response::ServiceDraft` (entries with their match state and candidates);
  `ServiceRequest::CreateFromDraft { draft }` writes the service. The UI
  never talks to the model. ts-rs generates the types as for every other
  message.
- UI: a dialog off Plan's "New service" button (T49).

## Where the code disagrees

- `ServiceRequest::Create` takes only `title`; nothing sets `starts_at`.
  The date needs the scheduling change.
- No request sets an item's `notes` or a song's `arrangement`, so the key
  ("in G") has nowhere to go through today's API. `CreateFromDraft` writes
  the whole service at once, which sidesteps it, but Plan will want
  `ServiceRequest::UpdateItem` anyway.
- The team has no default translation; `LookupPassage` takes one per call.
  A device or team setting is needed to pick one for scripture items.
- `library::search` returns a `score`, but nothing says what score counts
  as a confident match. The resolver needs that threshold, set from the
  evaluation set.

## Risks

- **Wrong matches.** "Reasons" could match the wrong song. The review step
  is the defence: every row shows what it matched, and nothing is created
  until the person presses Create.
- **Instructions in the text.** A pasted message could contain "ignore the
  above and…". The model's output is only a list of entries in a fixed
  schema and it calls nothing, so the worst case is a wrong draft, which
  the person sees before creating it.
- **Slow machines.** If the probe finds a runtime but a test request takes
  over 20 s, the dialog falls back to the line parser rather than leaving
  the person waiting.

## Later

An open-ended assistant ("draft next month's services", "what haven't we
sung since Easter?") that works while nobody watches would need more: its
own team member and key with a suggest-only `Role::Assistant` (today a
Planner may write songs, themes and decks too), a `DocKind::Suggestion`
document holding Automerge changes that a person accepts and re-authors,
a way for the repo to write under a second actor, and member removal in
`TeamRequest`. It would reuse this decision's runtime, probe and
live-safety rule. Not proposed now.

Cloud models (Claude, ChatGPT, Gemini) through an MCP server were
considered first and dropped: they send the church's library and notes off
the machine, and the remote versions need the relay to read and answer for
the team's documents (CD-4).

## Open questions

1. Existing runtime only, or also embed llama.cpp with an opt-in download?
2. 1B or 4B class as the default, once the evaluation set exists?
3. Should "not found" songs offer to create a blank song with that title,
   or stay header placeholders?
