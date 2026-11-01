// What ops and commands share: opening things, toasts, the announcer.

// A form's named fields shadow its own properties and methods (`form.method` is the
// field called "method", `form.hasAttribute` the one called "hasAttribute"), whatever
// the field's author meant. Everything the runtime reads off a form goes through the
// prototype, so no field name can change what a form does.
const E = Element.prototype;
/** @param {Element} el @param {string} name */
export const attr = (el, name) => E.getAttribute.call(el, name);
/** @param {Element} el @param {string} name */
export const has = (el, name) => E.hasAttribute.call(el, name);
/** @param {Element} el @param {string} name @param {string} value */
export const set = (el, name, value) => E.setAttribute.call(el, name, value);
/** @param {Element} el @param {string} name */
export const unset = (el, name) => E.removeAttribute.call(el, name);
/** @param {Element} el @param {string} name @param {boolean} on */
export const toggle = (el, name, on) => E.toggleAttribute.call(el, name, on);
/** @param {Element} el @param {string} selector */
export const all = (el, selector) => E.querySelectorAll.call(el, selector);
const elements = /** @type {(this: HTMLFormElement) => HTMLFormControlsCollection} */ (Object.getOwnPropertyDescriptor(HTMLFormElement.prototype, "elements")?.get);
/** The field called `name` in `form`. @param {HTMLFormElement} form @param {string} name */
export const field = (form, name) => elements.call(form).namedItem(name);

/** @param {Element} el */
export function openTarget(el) {
  if (el.nodeName === "DIALOG") {
    if (!/** @type {HTMLDialogElement} */ (el).open) /** @type {HTMLDialogElement} */ (el).showModal();
  } else if (el.hasAttribute("popover")) {
    try { /** @type {HTMLElement} */ (el).showPopover(); } catch {}
  } else el.setAttribute("open", "");
}

/** @param {Element} el */
export function closeTarget(el) {
  if (el.nodeName === "DIALOG") /** @type {HTMLDialogElement} */ (el).close();
  else if (el.hasAttribute("popover")) {
    try { /** @type {HTMLElement} */ (el).hidePopover(); } catch {}
  } else el.removeAttribute("open");
}

/** @type {HTMLElement | undefined} */
let live;

/** Says `text` to assistive technology. @param {string} text */
export function announce(text) {
  if (!live?.isConnected) {
    live = document.createElement("div");
    live.setAttribute("data-slot", "announcer");
    live.setAttribute("aria-live", "polite");
    live.setAttribute("data-ui-persist", "");
    document.body.append(live);
  }
  live.textContent = text;
}

/** @type {HTMLElement | undefined} */
let toaster;

/** Shows a toast, in the top layer, above any open dialog. @param {string} tone @param {string} text */
export function toast(tone, text) {
  if (!toaster?.isConnected) {
    toaster = document.createElement("div");
    toaster.setAttribute("data-slot", "toaster");
    toaster.setAttribute("data-ui-persist", "");
    toaster.setAttribute("popover", "manual");
    document.body.append(toaster);
  }
  const item = document.createElement("div");
  item.setAttribute("data-slot", "toast");
  item.setAttribute("data-tone", tone);
  item.setAttribute("role", tone === "danger" ? "alert" : "status");
  item.textContent = text;
  toaster.append(item);
  try {
    toaster.hidePopover();
    toaster.showPopover();
  } catch {}
  setTimeout(() => {
    item.remove();
    if (toaster && !toaster.firstChild) {
      try { toaster.hidePopover(); } catch {}
    }
  }, tone === "danger" ? 8000 : 4000);
}
