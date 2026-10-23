// What the main screen shows when nothing is on air, and the church's logo
// screen, as pictures for Present's live preview. Call from a component:
// it keeps itself current with the settings and the screen.

import type { OutputConfig } from "./api";
import { effectiveIdle } from "./idle";
import { loadIdlePicture, loadLogoPicture } from "./idleThumb";
import { live } from "./state.svelte";
import { toHex } from "./themes";

export interface LiveLook {
  /** A picture of the idle screen; null for the plain background. */
  idle: string | null;
  /** The Logo screen's picture (null until a logo is chosen), background and ring colour; null with no brand. */
  brand: { logo: string | null; background: string; accent: string } | null;
}

export function useLiveLook(output: () => OutputConfig | null, width = 640): LiveLook {
  const look: LiveLook = $state({ idle: null, brand: null });

  $effect(() => {
    const setting = effectiveIdle(output()?.idle ?? null, live.settings?.idle ?? null);
    let current = true;
    loadIdlePicture(setting, width).then((r) => {
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
      if (current) look.brand = { logo, background: toHex(brand.primary), accent: toHex(brand.accent) };
    });
    return () => {
      current = false;
    };
  });

  return look;
}
