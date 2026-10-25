// Lampstand Remote's page (T100), served by the laptop at `GET /remote`.
// Built on its own by `npm run build:remote` into the server crate, so cargo
// needs no Node.

import { mount } from "svelte";
import "../lib/kit/theme.css";
import App from "./App.svelte";

mount(App, { target: document.getElementById("app")! });
