// What ops and commands share: opening things, toasts, the announcer.

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
