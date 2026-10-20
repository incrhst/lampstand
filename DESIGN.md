---
name: Lampstand
description: Free, open-source church projection, presented as the church bulletin laid open on the table.
colors:
  paper: "#f2c452"
  crease: "#d9a93a"
  ink: "#1d1a15"
  ink-soft: "#4a3d22"
  stamp: "#4c2a85"
  stamp-deep: "#36195f"
  table: "#2b211a"
  table-edge: "#1c1510"
  screen: "#0b0e16"
  screen-ink: "#f5f1e8"
  screen-gold: "#e9b949"
typography:
  display:
    fontFamily: "Old Standard TT, Iowan Old Style, Palatino Linotype, Palatino, Georgia, serif"
    fontSize: "min(5.75rem, 13.4cqi)"
    fontWeight: 700
    lineHeight: 0.92
    letterSpacing: "0.04em"
  subtitle:
    fontFamily: "Old Standard TT, Iowan Old Style, Palatino Linotype, Palatino, Georgia, serif"
    fontSize: "clamp(1.2rem, 2vw, 1.6rem)"
    fontWeight: 400
    lineHeight: 1.25
  headline:
    fontFamily: "Old Standard TT, Iowan Old Style, Palatino Linotype, Palatino, Georgia, serif"
    fontSize: "clamp(1.3rem, 2vw, 1.6rem)"
    fontWeight: 700
    letterSpacing: "0.16em"
  title:
    fontFamily: "Old Standard TT, Iowan Old Style, Palatino Linotype, Palatino, Georgia, serif"
    fontSize: "1.15rem"
    fontWeight: 700
  lead:
    fontFamily: "Old Standard TT, Iowan Old Style, Palatino Linotype, Palatino, Georgia, serif"
    fontSize: "clamp(1.15rem, 1.5vw, 1.3rem)"
    fontWeight: 400
    lineHeight: 1.5
  body:
    fontFamily: "Old Standard TT, Iowan Old Style, Palatino Linotype, Palatino, Georgia, serif"
    fontSize: "1.0625rem"
    fontWeight: 400
    lineHeight: 1.55
  label:
    fontFamily: "Courier Prime, Courier New, Courier, monospace"
    fontSize: "0.95rem"
    fontWeight: 700
    letterSpacing: "0.04em"
  note:
    fontFamily: "Courier Prime, Courier New, Courier, monospace"
    fontSize: "0.9rem"
    fontWeight: 400
    lineHeight: 1.6
  stamp:
    fontFamily: "Courier Prime, Courier New, Courier, monospace"
    fontSize: "0.82rem"
    fontWeight: 700
    letterSpacing: "0.14em"
  refs:
    fontFamily: "Courier Prime, Courier New, Courier, monospace"
    fontSize: "0.78rem"
    fontWeight: 400
    letterSpacing: "0.05em"
rounded:
  none: "0px"
  dot: "50%"
spacing:
  gutter: "clamp(22px, 4.2vw, 64px)"
  table-margin: "clamp(0px, 3.2vw, 48px)"
  stack-sm: "0.6rem"
  stack-md: "1rem"
  stack-lg: "1.6rem"
components:
  button-stamp:
    backgroundColor: "transparent"
    textColor: "{colors.stamp}"
    typography: "{typography.label}"
    rounded: "{rounded.none}"
    padding: "0.8em 1.2em"
  button-stamp-hover:
    backgroundColor: "{colors.stamp}"
    textColor: "{colors.paper}"
  button-stamp-small:
    textColor: "{colors.stamp}"
    rounded: "{rounded.none}"
    padding: "0.65em 1em"
  link-typed:
    textColor: "{colors.ink}"
    typography: "{typography.label}"
  link-typed-hover:
    textColor: "{colors.stamp-deep}"
  status-stamp:
    backgroundColor: "transparent"
    textColor: "{colors.stamp}"
    typography: "{typography.stamp}"
    rounded: "{rounded.none}"
    padding: "0.15em 0.55em"
  key-button:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    rounded: "{rounded.none}"
    width: "44px"
    height: "44px"
  key-button-hover:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.paper}"
  service-item-live:
    textColor: "{colors.stamp-deep}"
    padding: "1.05rem 0.85rem 1.1rem"
  service-item-live-time:
    backgroundColor: "{colors.stamp}"
    textColor: "{colors.paper}"
    padding: "0.05em 0.3em"
  projector-screen:
    backgroundColor: "{colors.screen}"
    textColor: "{colors.screen-ink}"
    rounded: "{rounded.none}"
  sheet:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.ink}"
    width: "1200px"
    padding: "{spacing.gutter}"
---

# Design System: Lampstand

## Overview

**Creative North Star: "The Bulletin Laid Open"**

The site is the church bulletin handed out at the door: one sheet of goldenrod copy paper, opened flat on a dark wooden table, printed in photocopier-black ink with a single violet rubber stamp. Everything on the sheet is something a church office could produce with a typewriter, a hymnal typeface, a ruler and a stamp pad. Section heads sit between rules, items run to their job along dotted leaders, a perforation marks the tear-off card, and a fold runs down the middle of the spread.

The one thing that is not paper is the product's own output: a black projector screen with warm white text and a gold safe-area outline. It sits on the sheet like a photograph pasted in, and it is the only dark surface on the page. The page refuses the category's dark hero, app screenshots and feature-card grids; it persuades by letting the order of worship actually run.

Density is that of a printed bulletin: generous margins, a calm reading measure, and type doing the work that boxes and shadows do elsewhere.

**Key Characteristics:**
- One sheet of goldenrod paper on a dark table; no other surface colours.
- Black ink for everything, violet stamp ink for actions and the live state only.
- Hymnal serif (Old Standard TT) over typed Courier Prime.
- Square corners everywhere; rules, dotted leaders and perforations instead of boxes.
- The projector screen is the only dark, product-coloured area.

## Colors

Three materials: goldenrod copy paper, photocopier toner, and one violet stamp ink, with a black projector screen pasted in.

### Primary
- **Rubber-Stamp Violet** (stamp): the single accent. Actions (stamped buttons), the live marker, the live time chip, link underlines, focus outlines, text selection and the "In development" status stamp. 6.47:1 on paper.
- **Deep Stamp Violet** (stamp-deep): link hover and the name of the live service item. 8.76:1 on paper.

### Neutral
- **Goldenrod Copy Paper** (paper): the whole sheet, and the text colour on violet or black fills.
- **Fold Crease** (crease): the darker paper of the fold; used for the scrollbar thumb.
- **Photocopier Ink** (ink): all body text, headings, rules, the double rules and the key buttons. 10.57:1 on paper.
- **Faded Toner** (ink-soft): secondary text (status line, notes, refs), dotted leaders and the ledger dashes. 6.46:1 on paper.
- **Dark Table** (table) and **Table Edge** (table-edge): the wood the sheet lies on; page background, browser theme colour and scrollbar track.

### Tertiary: the projector
- **Projector Black** (screen): the video frame and the live screen. Never used for page chrome.
- **Projection White** (screen-ink): slide text on the screen (17.1:1).
- **Safe-Area Gold** (screen-gold): small slide captions, slide footers, the lower-third rule and, at 60% alpha, the dashed safe-area outline.

Translucent ink is used for dividers rather than new greys: ink at 28% for spread borders, 35% for dotted list dividers, 6% for row hover; stamp at 9% for the live row wash. The sample slides inside the screen (the cream sermon deck, the slate stream camera) carry their own hard-coded colours because they depict a church's content, not this system.

### Named Rules
**The One Ink Rule.** Violet is stamp ink: it marks what you can press and what is live, and nothing else. No violet headings, panels or decoration.

**The Paper Is the Page Rule.** There is one surface colour on the sheet. Sections are separated by rules, folds and perforations, never by a different fill.

**The Pasted Screen Rule.** Dark surfaces belong only to the product's output (the video frame and the projector screen).

## Typography

**Display Font:** Old Standard TT (with Iowan Old Style, Palatino, Georgia)
**Body Font:** Old Standard TT
**Label/Mono Font:** Courier Prime (with Courier New, Courier)

**Character:** a hymnal serif for what the congregation reads, and the church office typewriter for everything administrative: times, notes, references, actions and the footer. Both are self-hosted woff2 in 400, 400 italic and 700.

### Hierarchy
- **Display** (masthead): uppercase, sized to its panel with container units, tight leading. Appears once.
- **Subtitle**: italic serif, balanced, directly under the masthead.
- **Headline** (section heads): bold uppercase, widely tracked, centred between two hairline rules.
- **Title** (service item names, subheads at 1.05rem italic bold).
- **Lead** (the offer paragraph): up to 34em measure.
- **Body**: 1.0625rem / 1.55; rubrics capped at 58ch, ledger lines and plain notices at 52ch.
- **Label** (typed actions and links): Courier Prime bold, uppercase on the main stamped button, sentence case on the small one and on links.
- **Note** (typed asides): Courier Prime at 0.9rem in faded toner.
- **Stamp** and **Refs**: small tracked uppercase for the status stamp; small tracked typed text for requirement IDs.

### Named Rules
**The Typewriter Rule.** Anything the church office would type (times, references, instructions, actions, the colophon) is set in Courier Prime. Anything the congregation reads aloud or follows is in the serif.

## Layout

A single centred sheet, max 1200px, lying on the table with a margin of `table-margin` on desktop and none on phones. Each spread is a two-column grid of equal panels split by the centre fold; panel padding is the fluid `gutter`. Panels are inline-size containers so the masthead scales to its panel.

Spreads stack: outside spread (masthead and the prelude film), inside spread (the Order of Worship with a sticky projector booth on the right), the tear-off Connection Card, the Notices back page, and a colophon under a double rule. The card and notice columns cap at 46rem and centre.

At 880px and below the bulletin folds back on itself: one column, no table margin, no sheet shadow or crease gradient, and spreads joined by a two-line inset fold mark. In the Order of Worship the projector booth sticks to the top of the viewport above the list, and each service item scrolls with a `scroll-margin-top` equal to the measured booth height so it is never hidden under it. At 480px item names take their own line with the leader running to the job on the second, and actions go full width.

Vertical rhythm uses `stack-sm`, `stack-md` and `stack-lg`; double rules and subheads sit at `stack-lg`.

## Elevation & Depth

Depth is physical, not interface: the only lifted object is the sheet itself, resting on the table. Everything printed on it is flat. Structure comes from rules, folds and perforations.

### Shadow Vocabulary
- **Paper on the table** (`box-shadow: 0 2px 4px rgba(0,0,0,0.35), 0 24px 60px -12px rgba(0,0,0,0.55)`): the sheet only, desktop only.
- **Centre fold** (a horizontal linear gradient 26px each side of the centre, darkening then lightening the paper): the sheet only, desktop only.
- **Folded seam** (`box-shadow: inset 0 1px 0 rgba(150,100,10,0.35), inset 0 3px 0 rgba(255,240,200,0.4)`): between spreads on phones.
- **Sticky booth lip** (`box-shadow: 0 8px 12px -10px rgba(29,26,21,0.5)`): under the stuck projector booth on phones, so the list visibly slides beneath it.

### Named Rules
**The Only Object Rule.** The sheet is the only thing with a cast shadow. Cards, buttons and panels on it are flat ink.

**The Fold Is the Only Gradient Rule.** The centre crease is the one gradient in the system, because it is a fold in the paper.

## Shapes

Square corners throughout (`rounded.none`); the only circle is the live dot. Form comes from print devices:
- **Double rule** (4px double ink): under the subtitle and above the colophon; also the border of stamped actions.
- **Hairline rules** flanking centred section headings.
- **Dotted leaders** (2px dotted faded toner) running from an item name to its job.
- **Dotted dividers** (1px, ink at 35%) between list rows.
- **Perforation** (2px dashed ink) across the top of the Connection Card, with rotated scissors.
- **Safe-area outline** (1px dashed gold) inset inside the projector screen.

## Components

### Buttons
- **Stamped action** (`button-stamp`): violet text and a 4px double violet border on bare paper, typed bold uppercase with a star icon. Hover inks the stamp solid: violet fill, paper text, the border turns paper and a 2px violet ring outside keeps the double edge. Active presses down 1px. Transitions are 180ms on the shared ease.
- **Small stamped action** (`button-stamp-small`): the same stamp in sentence case, used on the Connection Card.
- **Key** (`key-button`): 44px square, 2px ink border, transparent; hover fills with ink and paper icon. Previous and next arrows in the booth.

### Status stamp
- **Style** (`status-stamp`): a single 1.5px violet rule around small tracked uppercase typed text. Only the "In development" mark uses it.
- The single rule distinguishes a status mark from an action, which always carries the double rule.

### Links
- **Typed link** (`link-typed`): Courier Prime bold in ink, with a 2px violet underline offset 0.22em; hover turns the text deep violet.

### Cards / Containers
There are no cards. The Connection Card is a section of the sheet below a perforation, with rows joined to their actions by dotted leaders.

### Order of Worship (signature)
- Each service item is a full-width, square, transparent button: a typed time, the item name, a dotted leader, the italic job, a rubric and typed requirement refs.
- **Hover**: ink wash at 6%.
- **Live** (`aria-current="true"`): stamp wash at 9%, the time inverts to a solid violet chip with paper text, the name turns deep violet. 220ms transitions.
- The live item advances on scroll (an IntersectionObserver band), on click, with the booth keys, and with Left/Right arrows while focus is in the section.

### Projector screen (signature)
- A 16:9 black screen with a 6–10px black frame, square, with the dashed gold safe area. Slides cross-fade at 200ms; deck builds rise 0.4em at 260ms.
- Beneath it, the booth bar: a violet live dot, typed "LIVE" and the item name in a polite live region, then the keys.
- The film in the outside spread uses the same black frame.

### Disclosure
- Typed summary in faded toner with a violet chevron that rotates 90° when open; the text summary of the film lives here.

## Do's and Don'ts

### Do:
- **Do** keep the whole page on the goldenrod paper, with violet reserved for actions, the live state, link underlines and focus.
- **Do** separate sections with rules, dotted leaders, folds and perforations rather than fills or boxes.
- **Do** give every action the 4px double violet stamp border, and keep the single 1.5px rule for status marks only.
- **Do** set times, references, notes, actions and the colophon in Courier Prime, and reading text in Old Standard TT.
- **Do** keep corners square; the live dot is the only circle.
- **Do** show focus as a 3px violet outline offset 3px.
- **Do** honour reduced motion: transitions drop to zero and the deck build waits for an explicit step.

### Don't:
- **Don't** put a dark hero, app screenshots or a row of feature cards on the sheet; dark belongs only to the projector screen and the film frame.
- **Don't** add radius, gradients (other than the centre fold) or shadows on anything printed on the sheet.
- **Don't** use violet for headings, decoration or large fills at rest.
- **Don't** add a second accent colour; the slide colours inside the screen are sample content, not palette.
