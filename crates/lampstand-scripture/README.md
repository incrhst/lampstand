# lampstand-scripture

Scripture for Lampstand (PRD BI-1, BI-2):

- **References** (`Reference::parse`): free-typed references such as
  `Jn 3:16-18`, `1 Cor 13`, `Ps 23:1-4,6`, `Gen 1:1-2:3`, `Jude 3`. Book
  names match on common abbreviations, unique prefixes and ordinals
  (`II Kings`, `1st John`). Errors read as messages for the operator:
  "John has 21 chapters; there is no chapter 22", "There is no book called
  “Jhon”. Did you mean John?".
- **Passages** (`Translation::passage`): every verse a reference covers, with
  a clear error when a verse is not in the translation.
- **Slides** (`split`): a passage split across slides by a `Budget` of lines
  and characters per line, never mid-word, keeping verse numbers. A verse that
  fits on one slide is never split; longer verses break after a clause where
  possible. The renderer supplies the budget from the output's safe area
  (`Budget::from_rect`).
- **Importers** (`import`): OSIS, USFM, Zefania XML and OpenSong. Each returns
  the translation with its metadata (name, language, copyright, licence) and
  a report of anything left out: apocryphal or unknown books, duplicate or
  joined verses, unknown USFM markers.

Not yet: the OpenLP format (an SQLite database), and showing two
translations side by side (BI-3, P1).

## Translations and licences

Churches import the translations they hold a licence for. Lampstand ships
public-domain texts only. No full Bible is in the repository yet; tests use a
short King James Version excerpt (Genesis 1:1–5 and 2:1–3, Psalm 23,
John 3:16–18) in `tests/fixtures/`, in each supported format.

When we bundle texts, these are the sources:

| Text | Source | Status |
| --- | --- | --- |
| King James Version (1769) | eBible.org `eng-kjv2006` (USFM, OSIS) <https://ebible.org/find/details.php?id=eng-kjv2006>; CrossWire SWORD KJV module (OSIS) <https://crosswire.org/sword/modules/ModInfo.jsp?modName=KJV> | Public domain in most countries. In the United Kingdom the text is under Crown rights, administered by Cambridge University Press through Letters Patent; reproducing it there needs their permission beyond the limits they allow. |
| World English Bible | eBible.org `eng-web` (USFM, OSIS) <https://ebible.org/find/details.php?id=eng-web> | Dedicated to the public domain. "World English Bible" is a trademark: a changed text must not use that name. |

Check the copyright page in each download before bundling it, and keep the
copyright line in `TranslationMeta` so it can be shown on screen.
