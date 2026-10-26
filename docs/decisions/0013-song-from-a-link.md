# 0013. A song from a link: shared files and public-domain hymns, never lyric sites

- Status: accepted by the owner on 2026-10-25 (which links, SongSelect, CCLI); the details marked **proposed** wait for approval when T125 is built
- Date: 2026-10-25

## Question

A worship leader often has a song's words somewhere online: a Google Doc the team shares, a chart in Dropbox, a hymn on a public-domain site, or a page on a lyric site. Pasting the link should bring the song in, as [T113](../TICKETS.md) does for a backing track. Which links may Lampstand read, what happens with a SongSelect link, and how does CCLI reporting apply?

The words of most worship songs are under copyright. A church's CCLI licence lets it project a covered song, whatever copy of the words it starts from, as long as it reports what it uses. That licence does not cover how a website's words are taken: lyric sites (Genius, AZLyrics, Musixmatch, Lyrics.com and others) forbid copying their pages in their terms, and many license the words from publishers only for display on their own site.

## Decision

1. **Lampstand reads links to the church's own files, and to public-domain hymns.**
   - **Shared documents and files:** a Google Doc (read as plain text through its export link), a Google Drive, Dropbox or OneDrive file, and a plain `https://` link to a song file.
   - **Song files** are read by their kind: plain text through Paste lyrics (`songs::paste`, T114), ChordPro through the ChordPro importer (T68), OpenLyrics and OpenSong through their importers. One link brings in one song.
   - **Public-domain hymns** from a short list of sites whose terms allow it (**proposed**: hymnary.org's text pages, if its terms allow it when T125 is built; otherwise none). Only texts the site marks as public domain are taken.
2. **Lyric sites are refused by their address, before any request.** The same way T113 refuses streaming links: "Lampstand can't take words from lyric sites: their terms don't allow it. Copy the words from your church's own chart, or from SongSelect, and use Paste lyrics." The list is in one place in `connectors::links`, beside the streaming hosts. Any other web page (one that is not a file and not on the list) says which links work. Lampstand never reads an arbitrary web page.
3. **A SongSelect link is refused too, with the way that works.** SongSelect pages need the church to be signed in, and CCLI gives API access only to partners. So: "Open the song in SongSelect, then copy the words into Paste lyrics, or download the file and add it with Import songs." Both already keep the authors, the copyright and the CCLI number. Signing in to SongSelect from Lampstand waits for a CCLI partnership (PRD risk table).
4. **The person checks the song before it is saved.** A link opens the same check as Paste lyrics: the sections, the title and every guess as a warning, then Save (and add to the open service). Nothing from a link reaches the library or the screens unseen.
5. **CCLI: ask for the number, never block.** A song saved from a link remembers the link it came from. If no CCLI number came with it, the check asks for one ("CCLI song number"), with "Public domain" as the other answer, which sets the copyright line to "Public domain". It can be left empty and filled in later; the song then shows "No CCLI number" in the library until someone adds one. Once CCLI reporting (T38) is built, a song from a link is reported like any other song. Until then, the check says "Only add words your church has the right to use.", as Paste lyrics does.
6. **Where a song came from is one field, shared with the library MCP server.** `Song::source` (a link, an assistant, or a paste) is a contract change to `core`, written only when set, so songs saved before read as having none. Decision [0011](0011-mcp-for-the-library.md) (proposed) needs the same field for "added by an assistant". Whichever ticket lands first adds it.

## Options considered

- **Any web page.** Read the page's text and run Paste lyrics on it. It is simple and catches every site, but it takes words from lyric sites against their terms, and the church takes on that risk without knowing. Rejected.
- **A short allow-list only.** Safe, but most churches keep charts in Drive or Dropbox, and an allow-list leaves those out. The chosen rule keeps the list for public-domain sites only.
- **Signing in to SongSelect inside Lampstand.** Best for the church, but it needs a CCLI partnership we don't have.
- **Requiring a CCLI number to save.** It stops a volunteer on Sunday morning when the number isn't to hand, and the song is projected whatever the library says. Asking, and showing what's missing, gets the number without blocking.

## Not decided here

- A whole folder of charts from one link.
- Watching a shared document for changes, as T41 does for decks.
- Chords from a document whose chords sit on a line above the words: that is T116 (Paste lyrics keeps chords), and it applies here once built.
