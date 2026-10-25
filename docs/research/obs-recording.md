# Driving OBS's recording for the sermon

Research for [#237](https://github.com/incrhst/lampstand/issues/237), part of the [Recording the sermon map](https://github.com/incrhst/lampstand/issues/233). It decides the OBS part of T121 (decision 0012, point 2). Checked on 2026-10-25 against obs-websocket `master` (5.7.x) and OBS Studio `master` (33.x).

## The answer in short

- `StartRecord` answers "success" before OBS has started. A full disk or a bad path is never reported in the answer. Lampstand must wait for `RecordStateChanged` with `OBS_WEBSOCKET_OUTPUT_STARTED`.
- The saved file's path comes from three places: `RecordStateChanged` on `STARTED` and on `STOPPED` (every OBS that Lampstand supports, 28 and later), and the `StopRecord` answer. `GetRecordStatus` never gives it.
- `CreateRecordChapter` needs OBS 30.2 or later and a Hybrid MP4 recording (Hybrid MOV too from OBS 32.0). Any other format answers error 702. An OBS older than 30.2 answers 204 (unknown request).
- The path is a path on OBS's computer. A Lampstand on another computer can show it and keep it, but cannot open, check, move or trash the file.
- **Recommendation:** start and stop OBS only when OBS was idle. When OBS is already recording, leave it alone and keep the sermon's start and end as offsets into OBS's file. Add a chapter as a bonus when OBS allows it. Never pause or split someone else's recording.

## The calls and events

From the [protocol docs](https://github.com/obsproject/obs-websocket/blob/master/docs/generated/protocol.md) and the [record request handler](https://github.com/obsproject/obs-websocket/blob/master/src/requesthandler/RequestHandler_Record.cpp).

| Name | Added | Fields | Notes |
| --- | --- | --- | --- |
| `GetRecordStatus` | 5.0.0 | `outputActive`, `outputPaused`, `outputTimecode`, `outputDuration` (ms), `outputBytes` | No path. |
| `StartRecord` | 5.0.0 | none | Error 500 `OutputRunning` if already recording. Otherwise success at once; the start happens later ("TODO: Call signal directly to perform blocking wait" in the source). |
| `StopRecord` | 5.0.0 | `outputPath` | Error 501 `OutputNotRunning` if not recording. The path is read from the output's settings when the stop is asked for. |
| `RecordStateChanged` (event) | 5.0.0 | `outputActive`, `outputState`, `outputPath` | States: `STARTING`, `STARTED`, `STOPPING`, `STOPPED`, `PAUSED`, `RESUMED`. |
| `RecordFileChanged` (event) | 5.5.0 | `newOutputPath` | Sent when a file split starts a new file. |
| `ToggleRecordPause`, `PauseRecord`, `ResumeRecord` | 5.0.0 | none | See below. |
| `SplitRecordFile` | 5.5.0 | none | Error 702 "Verify that file splitting is enabled in the output settings." |
| `CreateRecordChapter` | 5.5.0 | `chapterName` (optional) | Error 702 "Verify that the output being used supports chapter markers." |
| `GetRecordDirectory` / `SetRecordDirectory` | 5.0.0 / 5.3.0 | `recordDirectory` | Set changes OBS's saved setting. |

All of these events come under the `Outputs` subscription (bit 6), which Lampstand already asks for.

### Which versions send `outputPath`

- obs-websocket 5.0.0 sent it on `STOPPED` only ([4201664](https://github.com/obsproject/obs-websocket/commit/4201664c7)).
- 5.0.1 added it on `STARTED` too ([16f3ceb](https://github.com/obsproject/obs-websocket/commit/16f3ceb99)). The [current source](https://github.com/obsproject/obs-websocket/blob/master/src/eventhandler/EventHandler_Outputs.cpp) does the same. Every other state sends `null`.
- The generated protocol docs still say "if record stopped. `null` otherwise". The source is right and the docs are out of date.
- OBS 28.0.0 bundles obs-websocket 5.0.2 (the `plugins/obs-websocket` submodule at the `28.0.0` tag). So every OBS that Lampstand accepts (28 and later) sends the path on `STARTED` and `STOPPED`. Bundled versions: OBS 29.0 has 5.1.0, 30.0 has 5.3.3, 30.2 has 5.5.1, 31.0 has 5.5.4, 32.0 has 5.6.3, 33.0 has 5.7.6.
- The path comes from the output's `url` or `path` setting ([`GetLastRecordFileName`](https://github.com/obsproject/obs-websocket/blob/master/src/utils/Obs_StringHelper.cpp)). With "Custom Output (FFmpeg)" sent to a URL, it is that URL, not a file.
- After a file split, `STOPPED` gives the last file. The earlier files only appear in `RecordFileChanged` (OBS 30.2 and later).

## What `StartRecord` does in awkward cases

From OBS's [`OBSBasic_Recording.cpp`](https://github.com/obsproject/obs-studio/blob/master/frontend/widgets/OBSBasic_Recording.cpp), [`OBSBasic_Streaming.cpp`](https://github.com/obsproject/obs-studio/blob/master/frontend/widgets/OBSBasic_Streaming.cpp) and [`SimpleOutput.cpp`](https://github.com/obsproject/obs-studio/blob/master/frontend/utility/SimpleOutput.cpp).

- **While streaming.** Recording is a separate output and starts normally. If the recording uses the stream's encoder ("Same as stream" in Simple mode, or "Use stream encoder" in Advanced), it cannot be paused.
- **OBS set to record when streaming.** With "Automatically record when streaming" on, OBS starts recording with the stream. Lampstand then finds it already recording and must leave it alone. If "Keep recording when stream stops" is off, OBS stops the recording when the stream stops, even one that Lampstand started.
- **Bad recording path.** OBS shows "Bad File Path" on its own screen and does nothing else. No event is sent. `StartRecord` still answered success.
- **Disk nearly full (under 50 MB) at start.** OBS shows "Insufficient disk space" and does nothing else. No event. `StartRecord` still answered success.
- **Disk fills while recording.** OBS checks once a second, stops at under 50 MB, and shows the same message. Lampstand gets a `STOPPED` it did not ask for, with the path.
- **Output fails to start** (an encoder or muxer error). OBS sends `STARTING`, then shows "Failed to start recording" with the reason. A `STOPPED` may or may not follow, so Lampstand must not wait for one.
- **Unsupported format** (for example, more audio tracks than the format allows). OBS shows "The output format is either unsupported or does not support more than one audio track." The output stops.
- **Remux to MP4.** With "Automatically remux to mp4" on, `outputPath` is the original file (for example `.mkv`). The `.mp4` is written beside it after the stop, and no event says so.
- The messages are modal boxes on OBS's computer. If that is another computer, nobody at Lampstand sees them.

So Lampstand should treat `StartRecord` as a request, wait about 10 seconds for `STARTED`, and if it does not come, check `GetRecordStatus` and say "OBS didn't start recording. Check OBS." It should never take a failed start as "OBS was already recording".

## Pause and split

- **Pause.** OBS cannot pause a recording that shares the stream's encoder. In that case OBS's frontend quietly does nothing, but `PauseRecord` still answers success (it only checks whether OBS is already paused). So a pause cannot be trusted. Pausing someone else's whole-service recording would also break "never stop a recording it did not start". Not useful.
- **Split.** `SplitRecordFile` (OBS 30.2 and later) needs "Automatic file splitting" turned on in OBS's Advanced output settings, or it fails. It would cut the church's whole-service file into pieces they did not ask for. Not useful by default.

## Chapters

- `CreateRecordChapter` arrived in obs-websocket 5.5.0, bundled with OBS 30.2.0. The OBS frontend call is marked "versionadded:: 30.2" in the [frontend API docs](https://github.com/obsproject/obs-studio/blob/master/docs/sphinx/reference-frontend-api.rst).
- Only OBS's own MP4 output has an `add_chapter` handler ([`mp4-output.c`](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-outputs/mp4-output.c)). That is the "Hybrid MP4" format, and "Hybrid MOV" since OBS 32.0 ([5f2a4e9](https://github.com/obsproject/obs-studio/commit/5f2a4e976)). OBS's menu item reads "Add Chapter Marker (Hybrid MP4/MOV only)".
- With MKV, plain MP4, MOV, FLV, TS or Custom FFmpeg, the call answers error 702 with the comment above. It also fails (702) while the recording is paused, and with 501 when OBS isn't recording. On OBS before 30.2 the request is unknown (204).
- Chapter times were wrong after a file split until OBS 32.1 ([6076ada](https://github.com/obsproject/obs-studio/commit/6076ada56)).
- A chapter does not change the file's length or split it. Video editors and players (VLC, QuickTime) show it. It is a good mark, but only some churches will have the format, so it cannot be the only mark.

## When OBS is on another computer

- `outputPath` is a path on OBS's computer, written the way that system writes paths. Lampstand can store it, show it, and copy it to the clipboard.
- obs-websocket has no request to read, list, check, rename or delete a file. A remote Lampstand cannot check the file exists, read its size, play it, trim it or move it to the Trash.
- Lampstand already knows whether OBS is on this computer (`is_local` in `app/src-tauri/src/obs_control.rs`). It must use that, not "does this path exist here": two Macs with the same user name can have the same path.
- "Show in folder" only makes sense when OBS is local. Otherwise the Recordings page says "Saved on {OBS computer}: {path}".
- Naming OBS's file after the sermon would need `SetRecordDirectory` or a change to OBS's filename format through profile parameters. Both change the church's OBS settings for good. Don't.
- When Lampstand connects while OBS is already recording, it cannot learn the current path (`GetRecordStatus` has none) until a `RecordFileChanged` or the `STOPPED` event. If Lampstand is not connected at the stop, the path is never known.

## What our `obs.rs` parses today

In `crates/lampstand-connectors/src/obs.rs`:

- `start_record()` and `stop_record()` throw away the answer (`.map(drop)`). The `outputPath` from `StopRecord` is lost.
- `ObsEvent::RecordStateChanged { active, state }` drops `outputPath`.
- `RecordStatus` reads `outputActive`, `outputPaused`, `outputTimecode`. It does not read `outputDuration` (milliseconds, easier than parsing the timecode) or `outputBytes`.
- `RecordFileChanged` falls through to `ObsEvent::Other` with its data, so it is not lost, but nothing reads it.
- No `CreateRecordChapter`. `codes` has 500, 501, 600 and 601, but not 204 or 702.
- The Outputs subscription is on, and `obs_control.rs` only uses `RecordStateChanged` to refresh its status view.

T121 needs: `outputPath` on the event (as `Option<String>`), the `StopRecord` answer's path, `outputDuration` in `RecordStatus`, a `RecordFileChanged` event, `create_record_chapter(name)`, and the codes 204 and 702. These are small, additive changes.

## Recommendation for T121

**When OBS is idle at the sermon's start** (and "Also record video in OBS" is on):

1. Send `StartRecord`. On 500, OBS started recording in the meantime: treat it as "already recording" below.
2. Wait up to 10 seconds for `RecordStateChanged` `STARTED`. Keep its `outputPath`. If it does not come, check `GetRecordStatus` once, then tell the volunteer "OBS didn't start recording. Check OBS." Keep recording audio either way.
3. At the sermon's end, send `StopRecord`, but only if this Lampstand started the recording and it is still running. Keep the answer's `outputPath`, and replace it with the one in the `STOPPED` event if they differ.
4. If OBS stops by itself (disk full, the stream stopped, a person pressed Stop), keep the path from `STOPPED`, note that OBS stopped early, and say so. Don't start OBS again by itself.
5. Add each `RecordFileChanged` path to the list of files.

**When OBS is already recording at the sermon's start:**

1. Never stop, pause or split it.
2. Read `GetRecordStatus` `outputDuration` at the sermon's start and end. Keep them as offsets into OBS's file. That tells the volunteer where to cut better than clock times, because OBS's file did not start at a known clock time. If OBS's recording paused in between, the offsets are off by the pause; note `outputPaused`.
3. Also try `CreateRecordChapter` with the item's title at the start (and "After the sermon" at the end). Ignore 702 and 204. Keep whether it worked.
4. The path is known only if Lampstand sees `STOPPED` or `RecordFileChanged` later. Update the entry in `recordings.json` then, even if Lampstand's audio recording ended long before.

**What `recordings.json` keeps for OBS** (one optional `obs` object per recording):

- `started_by_lampstand`: true or false. Decides who stops it, and how to read the offsets.
- `files`: the paths OBS reported, in order (more than one after a split). Empty when unknown. Each path as OBS wrote it, never changed.
- `host`: the OBS computer's name as set in Setup, and `local`: whether OBS was on this computer when it recorded. "Show in folder" uses `local`, never a check of the path.
- `start_ms` and `end_ms`: the sermon's place in OBS's file (0 and the length when Lampstand started it).
- `chapter`: whether a chapter mark was written.
- `stopped_early`: set when OBS stopped before the sermon ended, with a reason when one is known (`disk_full`, `not_started`, `stopped_in_obs`).

Not kept: the file size, whether it still exists, or a remuxed `.mp4`. Lampstand cannot know these for a remote OBS. For a local OBS it can look when the Recordings page opens.
