// A component whose body must be sealed before this file runs, and that draws only once something it
// waited for has come: what it draws then is no hook either.
export const contract = 1;

/** Whether the body was sealed when this module was evaluated. */
/** @type {any} */ (globalThis).sealedAtLoad = document.getElementById("late-body")?.hasAttribute("data-ui-stored");

/** @param {HTMLElement} el @param {any} props @param {any} ui */
export async function mount(el, props, ui) {
  await Promise.resolve();
  const part = document.createElement("div");
  part.setAttribute("data-ui-own", "");
  part.innerHTML = `<button id="late-planted" data-ui-click='[["toggle","menu"]]'>Jalandhar</button>`;
  ui.find('[data-slot="proof:late-body"]').append(part);
}
