// A component that draws a line into its body, and notes each call it gets in `globalThis.seen`, by its
// element's id. What it is given to work with is kept in `globalThis.uis`, by the same id.
export const contract = 1;

/** @param {HTMLElement} el @param {any} props @param {any} ui */
export function mount(el, props, ui) {
  const g = /** @type {any} */ (globalThis);
  const seen = (g.seen ||= []);
  (g.uis ||= {})[el.id] = ui;
  seen.push({ id: el.id, call: "mount", props });
  const line = document.createElement("p");
  line.setAttribute("data-slot", "proof:line");
  line.textContent = `Ludhiana, step ${props?.step}`;
  ui.find('[data-slot="proof:pad-body"]')?.append(line);
  return {
    /** @param {any} next */
    update(next) {
      seen.push({ id: el.id, call: "update", props: next });
    },
    leave() {
      seen.push({ id: el.id, call: "leave" });
    },
  };
}
