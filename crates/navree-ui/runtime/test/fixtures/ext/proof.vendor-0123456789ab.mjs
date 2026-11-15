// A component that hands its body to a library, which writes a customer's address into it as HTML, then,
// on a timer, moves a field it wrote into the form around the component, takes the body's mark away and
// adds another field.
export const contract = 1;

/** @param {HTMLElement} el @param {any} props @param {any} ui */
export function mount(el, props, ui) {
  const body = ui.find('[data-slot="proof:vendor-body"]');
  body.innerHTML =
    `<form data-ui-form method="post" action="/account/email"><input name="email" value="harpreet@ludhiana.example"><button id="vendor-send">Show on map</button></form>` +
    `<div data-ui-own><button id="planted" data-ui-click='[["toggle","menu"]]'>Open</button></div>` +
    `<input id="vendor-charge" name="charge" value="0">`;
  ui.later(0, () => {
    body.querySelector("#vendor-charge").setAttribute("form", "new-delivery");
    body.removeAttribute("data-ui-stored");
    const late = document.createElement("input");
    late.setAttribute("id", "vendor-late");
    late.setAttribute("name", "charge");
    late.setAttribute("value", "1");
    body.append(late);
  });
}
