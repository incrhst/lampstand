// The Get started strip: four things that take a new install from "just
// opened" to "ready for Sunday", each ticked off by what the app can see.
// Kept free of Svelte and Tauri so node can test it.

export const WELCOME_TITLE = "Welcome to Lampstand";

/** What the strip can see. */
export interface Facts {
  /** Screens (windows and displays) that are on. */
  screensOn: number;
  /** How many songs are in the library, counted up to a few. */
  songs: number;
  /** The services, by title and how many items each has. */
  services: { id: string; title: string; items: number }[];
  /** The operator has opened a service for live at least once. */
  triedService: boolean;
}

export type StepId = "try" | "screen" | "songs" | "plan";

export interface Step {
  id: StepId;
  title: string;
  /** One line on what it is for. */
  hint: string;
  done: boolean;
}

/** The two hymns that come with the app: more than this means the church has added its own. */
export const SEED_SONGS = 2;

/** The service made for a new install, if it is still there. */
export const welcomeService = (services: Facts["services"]) => services.find((s) => s.title === WELCOME_TITLE && s.items > 0);

export function steps(f: Facts): Step[] {
  return [
    {
      id: "try",
      title: "Try the Welcome service",
      hint: "Go live with two hymns and a verse. Press Space to move on.",
      done: f.triedService,
    },
    {
      id: "screen",
      title: "Choose your screen",
      hint: "Pick the projector or TV the congregation will see.",
      done: f.screensOn > 0,
    },
    {
      id: "songs",
      title: "Add your songs",
      hint: "Drop in files from OpenLP, EasyWorship, SongSelect and more.",
      done: f.songs > SEED_SONGS,
    },
    {
      id: "plan",
      title: "Plan your first service",
      hint: "Build Sunday's order from songs, verses, slides and video.",
      done: f.services.some((s) => s.title !== WELCOME_TITLE && s.items > 0),
    },
  ];
}

export const allDone = (list: Step[]) => list.every((s) => s.done);
export const remaining = (list: Step[]) => list.filter((s) => !s.done).length;
