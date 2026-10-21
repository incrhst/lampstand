type Handler = (e: { payload: unknown }) => void;
const handlers = new Set<Handler>();
export async function listen(_name: string, h: Handler) {
  handlers.add(h);
  return () => handlers.delete(h);
}
export function emit(payload: unknown) {
  for (const h of handlers) h({ payload });
}
