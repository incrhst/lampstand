// Which Bible a reference is read from. The version typed after a reference
// ("James 3:11 ESV") is matched in the core, which answers with the
// translation it used; this is the choice for a reference that names none.

import type { Translation } from "./api/index";

/**
 * The translation to read from when none is typed: `current` (what the
 * operator picked a moment ago) if this device still has it, else the
 * default, else the first (translations arrive sorted by name). Empty when
 * there are none.
 */
export function chooseTranslation(translations: Translation[], preferred: string | null, current = ""): string {
  for (const id of [current, preferred]) {
    if (id && translations.some((t) => t.id === id)) return id;
  }
  return translations[0]?.id ?? "";
}

/** "KJV", or the name when the translation has no abbreviation. */
export function versionLabel(t: Translation): string {
  return t.abbreviation || t.name;
}
