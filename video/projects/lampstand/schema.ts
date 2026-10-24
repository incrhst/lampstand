import { z } from "zod";
import { base, coreScenes, Custom, makeVideoProps } from "@video/core";
import { ROLE_KEYS } from "./roles";

const Role = z.enum(ROLE_KEYS);

export const Quote = z.object({
  type: z.literal("quote"),
  text: z.string(),
  attribution: z.string().optional(),
  ...base,
});
export const Roles = z.object({
  type: z.literal("roles"),
  heading: z.string().optional(),
  show: z.array(Role).min(1).max(4).default([...ROLE_KEYS]),
  focus: Role.optional(), // one role large, with what it owns and needs
  ...base,
});
export const Devices = z.object({
  type: z.literal("devices"),
  heading: z.string().optional(),
  machines: z.array(z.object({
    name: z.string(),
    caption: z.string().optional(),
    kind: z.enum(["laptop", "projector", "desk", "phone"]).default("laptop"),
    roles: z.array(Role).max(4).default([]),
  })).min(1).max(4),
  link: z.string().optional(), // label on the line between machines
  ...base,
});
export const Diagram = z.object({
  type: z.literal("diagram"),
  src: z.string().startsWith("diagrams/"), // a file in video/public/diagrams (the SVGs from docs/diagrams)
  heading: z.string().optional(),
  caption: z.string().optional(),
  ...base,
});
export const Screenshot = z.object({
  type: z.literal("screenshot"),
  src: z.string().startsWith("shots/"), // a file in video/public/shots
  heading: z.string().optional(),
  ratio: z.number().positive().default(1.6), // width / height of the picture
  callouts: z.array(z.object({ x: z.number().min(0).max(100), y: z.number().min(0).max(100), label: z.string() })).max(4).default([]),
  ...base,
});

const Machine = z.object({ name: z.string(), kind: z.enum(["laptop", "projector", "desk", "phone"]).default("laptop") });
export const Handover = z.object({
  type: z.literal("handover"),
  role: Role,
  outcome: z.enum(["allow", "decline", "cut"]),
  holder: Machine, // has the job now
  asker: Machine, // wants it
  caption: z.string().optional(),
  ...base,
});

export const Scene = z.discriminatedUnion("type", [...coreScenes, Quote, Roles, Devices, Diagram, Screenshot, Handover, Custom]);
export const VideoProps = makeVideoProps(Scene);
