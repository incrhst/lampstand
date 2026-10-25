import { Title, Bullets, Stat, Cta, type Registry } from "@video/core";
import { Quote } from "./scenes/Quote";
import { Roles } from "./scenes/Roles";
import { Devices } from "./scenes/Devices";
import { Diagram } from "./scenes/Diagram";
import { Handover } from "./scenes/Handover";
import { Screenshot } from "./scenes/Screenshot";

export const registry: Registry = { title: Title, bullets: Bullets, stat: Stat, cta: Cta, quote: Quote, roles: Roles, devices: Devices, diagram: Diagram, screenshot: Screenshot, handover: Handover };
export const customRegistry: Registry = {};
