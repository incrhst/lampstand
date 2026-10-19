// The UI's only way to the core: one `api` command and one `api-event`
// stream, typed by the generated bindings in ./api (never edit those).
// Screens call `request` for a typed payload, or `send` when the answer
// is just Ok, and subscribe with `onEvent`.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Event, Request, Response } from "./api/index";

export type * from "./api/index";

/** A request the core answered with `Response::Error`. */
export class ApiError extends Error {}

type ResponseType = Response["type"];
type Payload<T extends ResponseType> =
  Extract<Response, { type: T }> extends { data: infer D } ? D : undefined;

/** Sends a request and returns the raw response. Never throws for `Error`. */
export function call(request: Request): Promise<Response> {
  return invoke<Response>("api", { request });
}

/**
 * Sends a request and returns the payload of the response type you
 * expect. Throws `ApiError` on `Response::Error` or any other type.
 */
export async function request<T extends ResponseType>(req: Request, expect: T): Promise<Payload<T>> {
  const response = await call(req);
  if (response.type === "Error") throw new ApiError(response.data.message);
  if (response.type !== expect) {
    throw new ApiError(`expected ${expect}, got ${response.type}`);
  }
  return (response as { data?: unknown }).data as Payload<T>;
}

/** Sends a request whose answer is `Ok` (live commands, most edits). */
export async function send(req: Request): Promise<void> {
  const response = await call(req);
  if (response.type === "Error") throw new ApiError(response.data.message);
}

/** Subscribes to pushed events. Call the returned function to stop. */
export function onEvent(handler: (event: Event) => void): Promise<UnlistenFn> {
  return listen<Event>("api-event", (e) => handler(e.payload));
}
