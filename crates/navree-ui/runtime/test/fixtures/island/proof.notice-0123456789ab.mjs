// An island script of the plugin `proof`, named under its namespace: its body must be sealed before this file
// runs. It writes into its body what a library might: a button with a command, a form field, and an element that
// names a component of a plugin's.
const g = /** @type {any} */ (globalThis);

g.noticeLoaded = true;
/** Whether the island's body was sealed when this module was evaluated. */
g.noticeSealedAtLoad = document.getElementById("notice-body")?.hasAttribute("data-ui-stored");

/** @param {HTMLElement} body @param {any} props @param {HTMLElement} island */
export function mount(body, props, island) {
  g.noticeMount = { body, props, island };
  const part = document.createElement("div");
  // A constant, written as markup on purpose: the test is what the seal does with it.
  // nosemgrep: navree-raw-html
  part.innerHTML =
    `<button id="notice-planted" type="button" data-ui-click='[["toggle","menu"]]'>Open</button>` +
    `<input id="notice-field" name="charge" value="0">` +
    `<div id="notice-ext" data-slot="proof:pad" data-ui-ext="/_navree/ui/ext/proof.pad-0123456789ab.js" data-ui-props="{}"></div>`;
  body.append(part);
}
