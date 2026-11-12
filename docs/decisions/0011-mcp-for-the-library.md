# 0011. An MCP server for the song library: opt-in, local, plan and read only

- Status: proposed (not implemented; tickets T115 and T116; for 0.9.x)
- Date: 2026-10-25

## Question

A worship leader often has the words of a song somewhere an assistant can reach (a licensed lyrics source, their own notes, a SongSelect download) and wants it in the service without retyping. Paste lyrics (T114) covers the person at the keyboard. Could Lampstand also expose an [MCP](https://modelcontextprotocol.io) server, so an assistant the church already uses can search the library, bring a song in and put it in a service?

## Decision

**Yes, as a small server inside the app, off until the church turns it on, reachable only from this computer, and limited to reading and planning.**

1. **The app owns the library, so the server goes through the app.** The library is open in the running Lampstand and one process owns it. A separate MCP process must not open it as well. The server is a thin layer that turns each tool call into the `Request` the app already answers (`handler::App::handle`), so the checks, the sync and the events are the same as for the screens.
2. **Local only, with a token.** It listens on the loopback address (a separate port from the church-network one that the remote and sync use), or on a socket the app owns, and every call carries a token the church creates in Setup. It is never on the church network. A device that wants it from elsewhere pairs the way every device does.
3. **Off by default.** A switch in Setup turns it on and shows the token and how to connect an assistant. Turning it off closes it. A church that never turns it on sees nothing and runs nothing.
4. **Tools, in this order of need.**
   - Read: `search_songs`, `get_song`, `list_services`, `get_service`.
   - Plan: `add_song` (title, author, copyright, CCLI number, key, sections, arrangement), `add_song_to_service`, `create_service`.
   - No live control. Nothing that goes live, clears the screens, plays or stops sound, touches OBS or hands a role over. An assistant plans; a person presents.
5. **Lampstand fetches no lyrics.** The tool stores what the assistant supplies, as the plan contract already says of plans ([plan contract](../plan-contract.md): "No lyrics"; the church's library supplies the words, so its licences and edits stay its own). Where the words come from is the assistant's job and the church's licence. Lampstand does not scrape, search or call a lyrics service.
6. **A song added this way is marked and checked.** It is saved with where it came from ("added by an assistant"), and Plan shows it as not yet checked until someone opens it and says it is fine (as a plan from Notion is a draft the person checks). The same reading as Paste lyrics (`songs::paste`) is used, so a block of text with headings becomes sections the same way, and the result comes back with its warnings.
7. **Writes are small and named.** Each write tool says what it changed in its answer ("Added “Grace and Peace” to the library (4 sections), and to Sunday service after item 3"), and none deletes or overwrites: adding a song whose title is already in the library makes a second song and says so. Changing or removing songs is not offered.

## Why

- A thin layer over `handle` is the least new surface: no second path to the library, and what the screens can refuse the server refuses.
- The risks are an assistant doing more than asked, and words reaching the screens that nobody has checked. Limiting it to plan and read keeps nothing on air; marking and checking keeps unchecked words off the screens.
- Not fetching lyrics keeps the church's licence question with the church. Lampstand does not log CCLI usage yet (T38), so it cannot tell a church what it used; saying so beside the switch is part of the ticket.

## What it costs

- A new listener and token to look after, and a Setup panel. The token is shown once, kept hashed, and can be replaced.
- Every tool is a promise. They are added one at a time, each with tests that a request which the screens would refuse is refused here too.
- Assistants differ in how they connect (a local command, or an address). The first ticket supports one (a local address and token) and documents it; a command that bridges to it is a follow-up.

## Not decided here

- Whether a church's own assistant should be able to read the whole library or only search it (a very large library is a lot to hand over; `search_songs` returns hits with a first line, `get_song` one song).
- Writing back to a service already on air. Not in the first version: `add_song_to_service` on a service that is open for live is refused, with the reason.
- Whether a Teams permission (T18) will one day say which people may turn this on.
