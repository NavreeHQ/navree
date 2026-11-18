// A plugin's island with `update(next)`: when new props come, it writes a command and a field into the body
// the island holds then, and records whether that body was already sealed when `update` ran.
/** @param {HTMLElement} _body @param {any} _props @param {HTMLElement} island */
export function mount(_body, _props, island) {
  const g = /** @type {any} */ (globalThis);
  return {
    /** @param {any} next */
    update(next) {
      const current = /** @type {HTMLElement} */ (island.querySelector("[data-slot=island-body]"));
      (g.updatingSeen ||= []).push({ city: next?.city, sealedAtUpdate: current.hasAttribute("data-ui-stored") });
      const part = document.createElement("div");
      // A constant, written as markup on purpose: the test is what the seal does with it.
      // nosemgrep: navree-raw-html
      part.innerHTML =
        `<button id="updating-button" type="button" data-ui-click='[["toggle","menu"]]'>Open</button>` +
        `<input id="updating-field" name="charge" value="77">`;
      current.append(part);
    },
  };
}
