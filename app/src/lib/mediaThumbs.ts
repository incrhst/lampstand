// Thumbnails of images and videos, asked of the core and remembered. A
// video's picture is its first frame that is not blank, found by the core;
// decoding one takes a moment, so only a couple are asked for at once and
// each file is asked for once however many rows show it.

export type Fetcher = (blob: string, name: string, width: number) => Promise<string>;

/**
 * A loader that asks `fetcher` for at most `parallel` thumbnails at a time
 * and remembers every answer, including a failure (null: the file is not on
 * this device, or cannot be decoded), so a row that cannot have one is not
 * asked again.
 */
export function thumbnailLoader(fetcher: Fetcher, parallel = 2) {
  const done = new Map<string, Promise<string | null>>();
  const waiting: (() => void)[] = [];
  let running = 0;

  function next() {
    while (running < parallel && waiting.length > 0) {
      running++;
      waiting.shift()!();
    }
  }

  return function load(blob: string, name: string, width: number): Promise<string | null> {
    const key = `${blob}/${width}`;
    let found = done.get(key);
    if (!found) {
      found = new Promise<string | null>((resolve) => {
        waiting.push(() => {
          fetcher(blob, name, width)
            .then(resolve, () => resolve(null))
            .finally(() => {
              running--;
              next();
            });
        });
        next();
      });
      done.set(key, found);
    }
    return found;
  };
}
