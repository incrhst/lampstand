// Plain-volunteer wording from docs/roles.md (0.8 design). One place, so every scene says the same thing.
export const ROLE_KEYS = ["projection", "operator", "stream", "audio"] as const;
export type RoleKey = (typeof ROLE_KEYS)[number];
export const roleInfo: Record<RoleKey, { name: string; job: string; owns: string; needs: string }> = {
  projection: { name: "Projection", job: "Runs the screens", owns: "The picture on the projector and the stage screens.", needs: "The computer wired to the screens. It can run on its own." },
  operator: { name: "Operator", job: "Runs the service", owns: "Next, back, and jumping to any item.", needs: "Needs Projection to be running." },
  stream: { name: "Stream", job: "Runs what goes online", owns: "OBS, the live stream, and its mutes.", needs: "Works even if the projector computer is away." },
  audio: { name: "Audio", job: "Runs the sound", owns: "Video sound, music and backing tracks.", needs: "Plays from the computer next to the mixer." },
};
