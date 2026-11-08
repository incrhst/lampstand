// How a text item's body splits into slides. Mirrors
// `lampstand_core::text::text_slides`, which is what the projector uses;
// this copy only counts and previews slides while the planner types.

/** The lines of each slide: a blank line, or a line of three or more
 * hyphens, starts a new one. */
export function splitText(body: string): string[][] {
  const slides: string[][] = [];
  let current: string[] = [];
  for (const raw of body.split(/\r?\n/)) {
    const line = raw.trim();
    if (line === "" || /^-{3,}$/.test(line)) {
      if (current.length) slides.push(current);
      current = [];
    } else {
      current.push(line);
    }
  }
  if (current.length) slides.push(current);
  return slides;
}

/** What the order list calls a text item: its title, else its first words. */
export function textTitle(title: string, body: string): string {
  const t = title.trim();
  if (t) return t;
  const first = splitText(body)[0]?.[0];
  if (!first) return "Text slide";
  return first.length > 40 ? `${first.slice(0, 40).trimEnd()}…` : first;
}
