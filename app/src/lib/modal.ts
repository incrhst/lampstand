// Keyboard behaviour for a modal dialog (T60): focus moves into the dialog
// when it opens, Tab and Shift+Tab stay inside it, and focus goes back to
// what had it (the button that opened the dialog) when it closes. Escape is
// left to each dialog, which already closes on it.

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** Where Tab goes from `at` (-1 when focus is outside) among `count` stops; null lets the browser move on its own. */
export function wrapTab(at: number, count: number, back: boolean): number | null {
  if (count === 0) return null;
  if (at === -1) return back ? count - 1 : 0;
  if (back && at === 0) return count - 1;
  if (!back && at === count - 1) return 0;
  return null;
}

/** A Svelte action for the dialog element. With "self" the dialog itself takes focus first (it needs tabindex="-1"), so a screen reader reads its name; otherwise its first control does, or the dialog when it has none yet. */
export function modal(node: HTMLElement, initial?: "self") {
  const previous = document.activeElement as HTMLElement | null;
  // Tab stops in order. Tab reaches only the chosen radio button of a group,
  // so the others are left out, or Tab from the last stop would leave.
  const stops = () =>
    [...node.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((el) => {
      if (el.getClientRects().length === 0) return false;
      if (!(el instanceof HTMLInputElement) || el.type !== "radio" || el.checked || !el.name) return true;
      return !node.querySelector(`input[type="radio"][name="${CSS.escape(el.name)}"]:checked`);
    });

  queueMicrotask(() => {
    if (node.contains(document.activeElement)) return;
    const first = initial === "self" ? node : stops()[0];
    (first ?? node).focus({ preventScroll: true });
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key !== "Tab" || e.defaultPrevented) return;
    const all = stops();
    const to = wrapTab(all.indexOf(document.activeElement as HTMLElement), all.length, e.shiftKey);
    if (to === null) return;
    e.preventDefault();
    all[to].focus();
  }
  node.addEventListener("keydown", onkeydown);

  return {
    destroy() {
      node.removeEventListener("keydown", onkeydown);
      if (previous?.isConnected) previous.focus({ preventScroll: true });
    },
  };
}
