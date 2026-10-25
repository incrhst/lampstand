# Which encoder exports the sermon

Research for [#235](https://github.com/incrhst/lampstand/issues/235), part of the
[Recording the sermon map](https://github.com/incrhst/lampstand/issues/233). It decides
T118 and whether decision 0012's FLAC fallback stays. Checked on 2026-10-25.

## Answer

Use `mp3lame-encoder` 0.2.5 (over `mp3lame-sys` 0.1.11, bundled LAME 3.100) for MP3,
`id3` for tags and `ebur128` for loudness. It built from source on this Mac for both
Apple architectures with nothing installed, and its own CI builds on Windows, macOS and
Linux. Drop the FLAC fallback from decision 0012: no common church or podcast host
needs it, and WAV export already covers "give me the lossless file". Fix one detail in
0012 and T118: the exception in `deny.toml` must allow `LGPL-3.0`, not
`LGPL-2.0-or-later`.

## MP3 encoder crates

| Crate | Version, date | Licence (declared) | Notes |
|---|---|---|---|
| [mp3lame-encoder](https://crates.io/crates/mp3lame-encoder) | 0.2.5, 2026-08-20 | `LGPL-3.0` | Safe wrapper. Mono, stereo, CBR/VBR, f32 and i16 input, LAME tag. 1.17 M downloads. |
| [mp3lame-sys](https://crates.io/crates/mp3lame-sys) | 0.1.11, 2025-12-04 | `LGPL-3.0` | Bindings plus bundled `lame-3.100/` source. Static only. 1.13 M downloads. |
| [lame](https://crates.io/crates/lame) / [lame-sys](https://crates.io/crates/lame-sys) | 0.1.3, 2017 | MIT; MIT/Apache/LGPL-2.1 | Not maintained. Links a system libmp3lame. |
| [shine-rs](https://crates.io/crates/shine-rs) | 0.1.5, 2026-09 | `LGPL-2.0` | Port of the fixed-point shine encoder. New, small, lower quality than LAME. `LGPL-2.0` is not allowed by `deny.toml`. |

No pure-Rust MP3 encoder is mature. Decision 0012 is right about that.

### Licences, file by file

- `mp3lame-encoder/LICENSE` and `mp3lame-sys/LICENSE`: the LGPL 3.0 text. Both
  `Cargo.toml` files say `license = "LGPL-3.0"`, the deprecated SPDX id for
  LGPL-3.0-only.
- `lame-3.100/COPYING`: the GNU Library GPL 2.0 text. `lame-3.100/LICENSE` is LAME's
  FAQ on using it under the LGPL.
- `lame-3.100/libmp3lame/*.c`: 20 of 22 files say "version 2 of the License, or (at
  your option) any later version" (LGPL-2.0-or-later). `gain_analysis.c` says 2.1 or
  later. `fft.c` has no licence line, only Ron Mayer's note that "it would be polite to
  give credit".
- `lame-3.100/mpglib/` (the decoder) is LGPL-2.0-or-later too. It is only built with the
  `decoder` feature, which T118 does not need.

All of this is compatible with Lampstand's GPL-3.0-or-later. The LGPL's relinking rule is
met because Lampstand publishes its whole source. LAME's FAQ also asks the app to credit
LAME and link to its site: add it to the About screen and the third-party notices.

### What `deny.toml` needs

On `main`, `deny.toml` allows `LGPL-3.0-or-later` only. I ran `cargo deny check
licenses` (cargo-deny 0.20.2) on a scratch crate with `main`'s `deny.toml`:

- As it is: **fails**. Both crates are rejected: "license is not explicitly allowed".
- With an exception allowing `LGPL-3.0-only`: still fails. The deprecated id is not
  matched by its new name.
- With an exception allowing `LGPL-3.0` for `mp3lame-sys` and `mp3lame-encoder`:
  **passes** (with a "deprecated license identifier" warning).

So T118 adds, beside the `webpki-root-certs` exception:

```toml
# LAME, the MP3 encoder (docs/research/sermon-encoder.md). The crates declare the
# deprecated id "LGPL-3.0" (LGPL-3.0-only); the bundled C is LGPL-2.0-or-later.
# Both are GPL-3.0 compatible. Allowed for these crates only.
[[licenses.exceptions]]
crate = "mp3lame-sys"
allow = ["LGPL-3.0"]

[[licenses.exceptions]]
crate = "mp3lame-encoder"
allow = ["LGPL-3.0"]
```

`LGPL-2.0-or-later`, as decision 0012 and T118 say, would not match anything.

### How it builds

From [`build.rs`](https://github.com/DoumanAsh/mp3lame-sys/blob/master/build.rs):

- **Windows:** compiles the 20 LAME `.c` files with the `cc` crate and LAME's
  `configMS.h`. Needs only MSVC, which the `windows-latest` runner has.
- **macOS and Linux:** not `cc`. It uses the `autotools` crate to run LAME's bundled
  `configure` and `make` (static, no shared library, no frontend, no decoder). Needs
  `sh`, `make` and a C compiler, all on the `macos-latest`, `ubuntu-latest` and
  `ubuntu-22.04` runners. Nothing else is installed.
- **Cross-compiling for macOS:** when the target differs from the host it passes
  `--host=x86_64-apple-darwin` or `arm-apple-darwin`. Fixed in
  [mp3lame-sys#9](https://github.com/DoumanAsh/mp3lame-sys/issues/9). That matters
  because `release.yml` builds `universal-apple-darwin` on an arm64 runner.

What I ran on this Mac (Apple M1, macOS 26, rustc 1.99.1):

- `cargo build` (aarch64-apple-darwin): built in 39 s from clean, with `id3`, `ebur128`
  and `flacenc` beside it.
- `cargo build --target x86_64-apple-darwin`: built. `lipo -info` shows each
  `libmp3lame.a` is the right architecture (arm64, x86_64).
- Windows and Linux were not built here. The crate's own CI
  ([rust.yml](https://github.com/DoumanAsh/mp3lame-sys/blob/master/.github/workflows/rust.yml))
  builds on `macos-latest`, `windows-latest` and `ubuntu-latest`, plus five Linux cross
  targets. Lampstand's CI is the real test.

### Maintenance

- One maintainer (DoumanAsh) for both crates. mp3lame-encoder: last release 2026-08-20,
  releases in April and August 2026, 18 stars, 0 open issues. mp3lame-sys: last release
  2025-12-04, 7 stars, 1 open issue (Android i686). Ten issues so far, all build issues
  on Android, iOS and macOS cross-compiling, all but one fixed.
- LAME itself is frozen at 3.100 (2017). That is acceptable for an encoder: most
  published LAME CVEs are in the frontend and decoder, which are not built. MP3 patents have
  expired.
- Risk: a bus factor of one. The `-sys` crate is small (a `build.rs` and bindings), so a
  fork is cheap if it is ever needed.

## Tags: `id3`

[id3](https://crates.io/crates/id3) 1.17.2 (2026-09-22), MIT, pure Rust, 12.9 M
downloads, [on Codeberg](https://codeberg.org/polyfloyd/rust-id3). It wrote ID3v2.4
title, album, artist and recording date to the test MP3 and read them back exactly.
Use it rather than LAME's own `set_id3_tag`, which takes raw bytes and a year only,
so names such as "Église" and a full date need care.

## Loudness: `ebur128`

[ebur128](https://crates.io/crates/ebur128) 0.1.10 (2024-10-26), MIT, pure Rust port of
libebur128 by Sebastian Dröge (GStreamer). 1.1 M downloads, 131 stars, last commit
2025-02. It is a finished port rather than a moving one.

- Measures integrated loudness (`Mode::I`), loudness range, sample peak and **true peak**
  (`Mode::TRUE_PEAK`, 4x oversampling as BS.1770 asks).
- Speed on the M1, release build, two hours of 48 kHz mono: 3.3 s for integrated
  loudness, 6.0 s with true peak as well.
- After one gain pass, the test file measured -16.00 LUFS. A real sermon may need a
  limiter as well to keep the true peak under -1 dBTP; plain gain does not do that.

Apple asks for "-16 LKFS, within ±1 dB, with true peak no higher than -1 dBFS"
([Apple audio requirements](https://podcasters.apple.com/support/893-audio-requirements)).
That matches T118.

## Speed

Two hours of 48 kHz mono noise (a worst case), Apple M1, release build:

| Step | Time |
|---|---|
| Loudness with true peak | 6.0 s |
| MP3 96 kbps mono, LAME `-q 5` | 33.0 s |
| MP3 96 kbps mono, LAME `-q 7` | 27.4 s |
| FLAC 16-bit (flacenc, default settings) | 14.9 s |

Mono passes T118's "two hours in less than a minute" on the M1. Stereo 128 kbps is about
twice the work, and the 2019 laptop is slower than an M1. T118 should measure stereo on
the 2019 laptop. If it is over, use `-q 7`, encode on two threads (one per half), or
change the target to "two hours in less than two minutes".

## Fallbacks

| Format | Encoder in Rust | Licence | Build |
|---|---|---|---|
| FLAC | [flacenc](https://crates.io/crates/flacenc) 0.5.1, pure Rust | Apache-2.0 | Nothing extra |
| Opus | [opus-rs](https://docs.rs/crate/opus-rs/latest) (pure-Rust port of libopus 1.6, 0.1.x); or `opus`/`audiopus_sys` over libopus | BSD-style | Pure Rust is young. The C binding needs CMake. |
| AAC | None in pure Rust. `fdk-aac` bindings. | Fraunhofer FDK licence, which the FSF counts as GPL-incompatible | Not allowed by `deny.toml`, and the licence rules it out. |

flacenc is maintained (last commit 2026-06-29, 41 stars). A two-hour mono FLAC is still
half the size of a 16-bit WAV (691 MB) or more (555 MB for the noise test), so it is no good for a website or podcast.

### Who takes each format

| Host | MP3 | WAV | FLAC | Opus | AAC/M4A |
|---|---|---|---|---|---|
| Apple Podcasts (RSS feed) | Yes | No | No | No | Yes (preferred) |
| Spotify for Creators | Yes | Yes | No | No | Yes |
| Buzzsprout | Yes | Yes | Not listed | OGG listed | Yes |
| SermonAudio | Yes | Yes (discouraged) | No | No | Yes |
| Planning Center Publishing (hosted sermon audio) | Yes, up to 100 MB | No | No | No | No |
| WordPress media library | Yes | Yes | Yes | Ogg only | Yes |

Sources: [Apple](https://podcasters.apple.com/support/893-audio-requirements),
[Spotify](https://support.spotify.com/us/podcasters/article/uploading-audio-files/),
[Buzzsprout](https://www.buzzsprout.com/help/188-file-types),
[SermonAudio](https://www.sermonaudio.com/new_details.asp?id=22739),
[Planning Center](https://www.planningcenter.com/changelog/publishing/11965670784667-new-hosted-sermon-audio),
WordPress core's `wp_get_audio_extensions()`. Apple Podcasts Connect also takes WAV and
FLAC for subscriber audio only, not for a feed.

MP3 is the only format every host takes. A FLAC export would be accepted by almost none
of them, so as a fallback it would only move the problem to the volunteer.

## Recommendation for T118

1. Use `mp3lame-encoder` 0.2.5 with default features (no decoder). Mono 96 kbps,
   stereo 128 kbps, CBR, as decision 0012 says.
2. Add the two `deny.toml` exceptions above, allowing `LGPL-3.0`. Correct the wording in
   T118 and decision 0012 point 11 (`LGPL-2.0-or-later` is wrong).
3. Tag with `id3` (ID3v2.4, UTF-8), not LAME's tag. Credit LAME in the About screen.
4. Measure with `ebur128` (`Mode::I | Mode::TRUE_PEAK`), apply gain to -16 LUFS, and
   limit so the true peak stays under -1 dBTP.
5. Keep WAV export as the lossless option. Measure stereo speed on the 2019 laptop.
6. If the crate fails on one CI system, fix or vendor the build (it is small) rather than
   ship a different format there.

## Decision 0012's FLAC fallback

Drop it. The build risk it covered did not show up here, and FLAC does not do the job:
Apple's feed, Spotify, SermonAudio and Planning Center do not take it, and it is nearly
as large as the WAV. If a fallback is ever needed, it is "trimmed WAV only" with a clear
message, which T118 already builds.
