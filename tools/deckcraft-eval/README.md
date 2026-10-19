# deckcraft-eval

Scratch harness behind [docs/research/deckcraft-evaluation.md](../../docs/research/deckcraft-evaluation.md).
It is not a workspace member, CI does not build it, and nothing in `crates/` depends on it.

- `src/main.rs`: for each `.pptx`, counts what the package holds by reading its XML directly
  (slides, text, shapes, pictures, video, audio, masters, transitions, animation effects, click
  steps, speaker notes), imports it with `deckcraft_pptx::import_checked`, counts what the
  DeckCraft model kept and what DeckCraft's player would click through, and prints one JSON line
  per deck. Panics are caught and reported.
- `examples/dump.rs`: prints one deck's transitions and animations as DeckCraft holds them.
- `fetch_corpus.sh`: downloads the 37 public decks (Apache POI and LibreOffice test data).
- `make_church_decks.py`: generates the four church-service source decks (public-domain hymn
  and WEB scripture text, images and video made with ffmpeg). Needs `python-pptx` 1.0.

```bash
./fetch_corpus.sh corpus
```

```bash
cargo run --release -- corpus/*/*.pptx > results.jsonl
```

The Keynote and LibreOffice variants of the church decks come from opening the generated
decks in Keynote 14.1 (AppleScript `open` then `export … as Microsoft PowerPoint`, presenter notes
re-added in Keynote because it rejects python-pptx notes slides) and from
`soffice --headless --convert-to 'pptx:Impress MS PowerPoint 2007 XML'` (LibreOffice 25.8).
