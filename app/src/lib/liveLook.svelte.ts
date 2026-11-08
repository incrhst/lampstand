// What the main screen shows when nothing is on air (and under Idle), and
// the church's logo for the logo bug, as pictures for Present's live preview. Call from a component:
// it keeps itself current with the settings and the screen.

import type { LogoBug, OutputConfig } from "./api";
import { effectiveIdle } from "./idle";
import { loadIdlePicture, loadLogoPicture } from "./idleThumb";
import { live } from "./state.svelte";
import { toHex } from "./themes";

export interface LiveLook {
  /** A picture of the idle screen; null for the plain background. */
  idle: string | null;
  /** The church's logo picture (null until a logo is chosen), colours and logo bug; null with no brand. */
  brand: { logo: string | null; background: string; accent: string; bug?: LogoBug } | null;
}

export function useLiveLook(output: () => OutputConfig | null, width = 640): LiveLook {
  const look: LiveLook = $state({ idle: null, brand: null });

  $effect(() => {
    const setting = effectiveIdle(output()?.idle ?? null, live.settings?.idle ?? null);
    let current = true;
    loadIdlePicture(setting, width, live.settings?.brand ?? null, live.settings?.default_theme ?? null).then((r) => {
      if (current) look.idle = r?.picture ?? null;
    });
    return () => {
      current = false;
    };
  });

  $effect(() => {
    const brand = live.settings?.brand ?? null;
    if (!brand) {
      look.brand = null;
      return;
    }
    let current = true;
    loadLogoPicture(brand.logo, width).then((logo) => {
      if (current) look.brand = { logo, background: toHex(brand.primary), accent: toHex(brand.accent), bug: brand.bug };
    });
    return () => {
      current = false;
    };
  });

  return look;
}
