import { coreDefaultSec, words } from "@video/core";

export function defaultSec(s: any): number {
  switch (s.type) {
    case "quote": return 3 + words(`${s.text} ${s.attribution ?? ""}`) / 2.6;
    case "roles": return s.focus ? 5.5 : 2.5 + s.show.length * 1.6;
    case "devices": return 3 + s.machines.length * 1.8;
    case "diagram": return 5 + words(s.caption) / 3;
    case "handover": return s.outcome === "cut" ? 11 : 9;
    case "screenshot": return 3.5 + s.callouts.length * 1.8;
    default: return coreDefaultSec(s) ?? 4;
  }
}
