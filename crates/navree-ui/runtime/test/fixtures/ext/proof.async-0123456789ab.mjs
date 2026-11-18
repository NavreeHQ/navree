// A plugin's component whose `mount` waits on `globalThis.slowExtGate`, then puts another body in place of the
// one it started with before `mount` finishes, with no `ui:swap` after.
export const contract = 1;

/** @param {HTMLElement} el */
export async function mount(el) {
  await /** @type {any} */ (globalThis).slowExtGate;
  const old = /** @type {HTMLElement} */ (el.querySelector("[data-ui-keep]"));
  const next = document.createElement("div");
  next.setAttribute("id", "slow-ext-next");
  next.setAttribute("data-slot", "proof:async-body");
  next.setAttribute("data-ui-keep", "");
  // A constant, written as markup on purpose: the test is what the seal does with it.
  // nosemgrep: navree-raw-html
  next.innerHTML =
    `<button id="slow-ext-button" type="button" data-ui-click='[["toggle","menu"]]'>Bathinda</button>` +
    `<input id="slow-ext-field" name="charge" value="88">`;
  old.replaceWith(next);
}
