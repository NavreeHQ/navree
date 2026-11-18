// A plugin's island whose `mount` waits on `globalThis.slowIslandGate`, then puts another body in place of the
// one it started with before `mount` finishes, with no `ui:swap` after.
/** @param {HTMLElement} body */
export async function mount(body) {
  await /** @type {any} */ (globalThis).slowIslandGate;
  const next = document.createElement("div");
  next.setAttribute("id", "slow-island-next");
  next.setAttribute("data-slot", "island-body");
  next.setAttribute("data-ui-keep", "");
  // A constant, written as markup on purpose: the test is what the seal does with it.
  // nosemgrep: navree-raw-html
  next.innerHTML =
    `<button id="slow-island-button" type="button" data-ui-click='[["toggle","menu"]]'>Patiala</button>` +
    `<input id="slow-island-field" name="charge" value="99">`;
  body.replaceWith(next);
}
