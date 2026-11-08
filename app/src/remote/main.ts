// Lampstand Remote's page (T100), served by the laptop at `GET /remote`.
// Built by `npm run build:remote` (which `npm run build` and `npm run dev`
// run too) into the server crate, and checked in so cargo needs no Node.

import { mount } from "svelte";
import "../lib/kit/theme.css";
import App from "./App.svelte";

mount(App, { target: document.getElementById("app")! });
