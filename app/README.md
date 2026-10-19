# app

The Tauri 2 desktop shell (`src-tauri/`) and the Svelte 5 + TypeScript web UI
(`src/`) for the operator and planning screens. How to run it is in the
top-level README; how it fits together is under "UI" and "Outputs render
natively on one render thread" in docs/ARCHITECTURE.md.

| Path | What it holds |
| --- | --- |
| `src-tauri/src/handler.rs` | App state and every request handler, without Tauri (tested in `src-tauri/tests/`) |
| `src-tauri/src/shell.rs` | The `api` command, the `api-event` stream, output windows |
| `src-tauri/src/import.rs` | Format detection for `ImportFiles` |
| `src-tauri/seed/` | Public-domain hymns imported on first run |
| `src/lib/api.ts` | The TypeScript client every screen uses |
| `src/lib/api/` | Types generated from `lampstand-api` (never edit) |
| `src/lib/kit/` | Shared components and design tokens: `theme.css`, `Icon` (our own SVG set in `icons.ts`), `Button`, `Segmented`, `ListRow`, `StatusPill`, `Dot`, `Banner`, `Placeholder` (marks data a ticket will back), `ProgressBar`, `Switch`, `StepRail`, `StatusBar`, `SlidePreview`, `Panel` |
| `src/routes/<screen>/` | One folder per screen (T47 to T55 replace the bare ones) |

`npm run check` type-checks, `npm test` runs the TypeScript unit tests.
