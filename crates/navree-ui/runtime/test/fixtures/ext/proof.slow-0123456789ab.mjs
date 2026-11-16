// A component whose script is on its way until the test lets it come (`globalThis.slowGate`), and that then
// draws a button with a hook into its body: the body the element holds by then, which a morph may have put
// in place of the one it had when its script was asked for.
await /** @type {any} */ (globalThis).slowGate;

export const contract = 1;

/** @param {HTMLElement} el @param {any} props @param {any} ui */
export function mount(el, props, ui) {
  const body = ui.find("[data-slot='proof:slow-body']");
  // A constant, written as markup on purpose: the test is what the seal does with it.
  // nosemgrep: navree-raw-html
  body.innerHTML = `<button id="slow-planted" data-ui-click='[["toggle","menu"]]'>Jalandhar</button>`;
}
