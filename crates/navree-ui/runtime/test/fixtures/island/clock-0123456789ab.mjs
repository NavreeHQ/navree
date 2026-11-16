// An island script of the app's own, with no namespace: its body is not sealed. It writes a form field and a
// button with a command beside what the server drew, and both act as the server's would.
const g = /** @type {any} */ (globalThis);

/** @param {HTMLElement} body @param {any} props @param {HTMLElement} island */
export function mount(body, props, island) {
  g.clockMount = { body, props, island };
  g.clockMounts = (g.clockMounts || 0) + 1;
  const part = document.createElement("div");
  // A constant, written as markup on purpose: the test is what the islands module does with it.
  // nosemgrep: navree-raw-html
  part.innerHTML =
    `<input id="clock-zone" name="zone" value="Asia/Kolkata">` +
    `<button id="clock-planted" type="button" data-ui-click='[["toggle","clock-menu"]]'>Zones</button>`;
  body.append(part);
}
