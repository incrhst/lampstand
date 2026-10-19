// What the kit shows for each kind of service item.

import type { ItemKind } from "../api";
import type { IconName } from "./icons";

export type KindName = "Header" | "Song" | "Scripture" | "Deck" | "Media" | "Countdown" | "Announcements";

export function kindName(kind: ItemKind): KindName {
  return Object.keys(kind)[0] as KindName;
}

export const kindIcon: Record<KindName, IconName> = {
  Header: "header",
  Song: "song",
  Scripture: "scripture",
  Deck: "deck",
  Media: "media",
  Countdown: "countdown",
  Announcements: "announcements",
};

export const kindLabel: Record<KindName, string> = {
  Header: "Section heading",
  Song: "Song",
  Scripture: "Scripture",
  Deck: "Slide deck",
  Media: "Media",
  Countdown: "Countdown",
  Announcements: "Announcements",
};
