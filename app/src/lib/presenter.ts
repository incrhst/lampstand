// What the presenter view says about where a deck stands: "Slide 3 of 8",
// "Build 2 of 3", and how long the deck has been on air.

/** "Slide 3 of 8" for the 0-based `slide` of `count`. */
export function slideCounter(slide: number, count: number): string {
  return `Slide ${slide + 1} of ${count}`;
}

/** "Build 2 of 3" after `step` of `steps` clicks; null on a slide without builds. */
export function buildCounter(step: number, steps: number): string | null {
  if (steps <= 0) return null;
  return step === 0 ? `${steps} ${steps === 1 ? "build" : "builds"} to go` : `Build ${step} of ${steps}`;
}

/** Elapsed time as a clock: "0:07", "12:34", "1:02:03". */
export function formatElapsed(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const two = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${two(m)}:${two(s)}` : `${m}:${two(s)}`;
}
