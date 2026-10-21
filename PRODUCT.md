# Product

<!-- impeccable:product-schema 1 -->

## Platform

web (the public landing page). The product itself is a desktop app for Windows, macOS and Linux.

## Stack

Landing page: static HTML and CSS with no build step, published on GitHub Pages at lampstand.dbain.com (custom domain; `site/CNAME` records it, and the repository's Pages settings hold it).

## Users

- **Church volunteers and staff choosing a projection tool:** worship leaders, tech volunteers, church admins and media or stream leads. Most use OpenSong, OpenLP, EasyWorship, ProPresenter or FreeShow today, or PowerPoint on its own.
- **The Sunday operator:** usually a volunteer, aged 15 to 70, who runs the system twice a month after under an hour of training.
- **Open-source contributors:** Rust and web developers who might help build it.

## Product Purpose

Lampstand is a free, open-source church projection app. It replaces OpenSong and matches the core of ProPresenter, EasyWorship and FreeShow. It runs a whole service (songs, scripture, sermon slides, announcements) from one laptop, works fully offline on Sunday morning, and is simple enough for a volunteer to run after 10 minutes of training.

## Positioning

- **Free and open source (GPL-3.0).** No subscription and no company cloud a church depends on.
- **PowerPoint and Google Slides play with their animations, video and speaker notes intact,** without PowerPoint installed on the projection laptop. Other tools flatten decks to images or remote-control PowerPoint.
- **The team plans the service together during the week.** Local-first sync means the projection laptop has everything offline on Sunday.
- **Per-screen title-safe calibration:** text never falls off a cropped TV or behind the band.
- **Lyrics and lower thirds go to OBS** as a browser source, with no plugin.
- **Switching from the church's current tool takes under an hour,** songs and sets included.

## Operating Context

The app is used live in services, on old laptops with several screens, often by volunteers. Planning happens during the week on other devices.

## Capabilities and Constraints

- **Status: pre-release, in active development.** There are no downloads or installers yet. Built so far: the core model, live engine, layout, the wgpu renderer with output windows and calibration, encrypted LAN sync, song and Bible importers, the PPTX parser, the OBS browser source server, and the Tauri app scaffold.
- **Requirements** are in docs/PRD.md; the plan is docs/ARCHITECTURE.md and docs/TICKETS.md.
- **Do not claim** that features are shipped, or give availability dates, download counts or user numbers.

## Brand Commitments

- **Name:** "Lampstand", used publicly. The PRD calls it a working codename, but the user chose to use it as-is.
- **Promo video:** 45 seconds, 1920×1080, from `promo/` (PR #6). It ends on a Lampstand end card. Competitor names appear only as plain text, never logos.

## Evidence on Hand

- **On hand:** the promo video and its rendered frames, the PRD's market scan, and the GitHub repository (github.com/incrhst/lampstand).
- **Absent, never to be fabricated:** no testimonials, church users, press, benchmarks or download counts.

## Product Principles

1. **Sunday must not fail.** Offline-first, and the output never shows a desktop or a frozen frame.
2. **Built for volunteers:** ten minutes of training, then running a service.
3. **Free, open and portable:** GPL-3.0, open formats, no lock-in.
4. **Honest about status:** say what exists and what's next.

## Accessibility & Inclusion

The product's operator UI targets WCAG 2.2 AA. The landing page should meet the same standard, with captions or a text summary for the video, since it has no narration.

## Primary call to action (landing page)

Star and follow the project on GitHub.
