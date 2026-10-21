// Quick add: put a song or a passage into the service that is open for live
// without leaving Present. These are the rules for where the new item goes
// and how it is found again afterwards; the palette is QuickAdd.svelte.

/** Where a quick-added item goes. "next" is straight after the item on air. */
export type Placement = "next" | "end";

/**
 * The index to insert at, given the service's items in order and the item
 * on air. "Next" is right after the live item, so it follows what is on air;
 * with nothing on air, or the live item gone, it joins the end.
 */
export function insertionIndex(order: string[], liveItem: string | null, where: Placement): number {
  if (where === "end" || liveItem === null) return order.length;
  const at = order.indexOf(liveItem);
  return at < 0 ? order.length : at + 1;
}

/** The ID in `after` that is not in `before`: the item just inserted. */
export function newItemId(before: string[], after: string[]): string | null {
  const known = new Set(before);
  return after.find((id) => !known.has(id)) ?? null;
}

/** What the keys do in the palette: how Enter, Shift+Enter and Ctrl/Cmd+Enter place and show. */
export function entryAction(e: { shiftKey: boolean; metaKey: boolean; ctrlKey: boolean }): { where: Placement; show: boolean } {
  if (e.shiftKey) return { where: "next", show: true };
  if (e.metaKey || e.ctrlKey) return { where: "end", show: false };
  return { where: "next", show: false };
}
