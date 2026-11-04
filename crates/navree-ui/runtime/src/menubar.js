// The menu bar module: context menus, menu bars and the navigation menu. The markup opens
// with no script where it can (a menu bar's buttons and the navigation menu's are popover
// buttons), so this adds what a browser doesn't: a context menu where the pointer is, the
// keys of the menu bar pattern, and hover and arrows for the navigation menu. The menus
// module gives every menu its own keys. It is delegated from `document` and reads through
// dom.js, so a morph needs no wiring and a name in stored markup can't shadow a method.
import { E, T, active, all, attr, byId, closest, has, holds, on, one, root, set } from "./dom.js";

const CTX = "[data-ui-context]", MENU = "[data-ui-menu]", BAR = "[data-ui-menubar]", TRIGGER = "[data-slot=menubar-trigger]";
const NAV = "[data-ui-navmenu]", TOP = "[data-slot=navigation-menu-trigger]", PANEL = "[data-slot=navigation-menu-content]";
const ITEM = "[role=menuitem],[role=menuitemcheckbox],[role=menuitemradio]";

/** @param {Element} node @param {string} state */
const matches = (node, state) => E.matches.call(node, state);
/** The element an attribute names by id, or null. @param {Element} node @param {string} name @returns {HTMLElement | null} */
const named = (node, name) => {
  const id = attr(node, name);
  return id ? byId(id) : null;
};
/** @param {Element} node */
const rtl = (node) => getComputedStyle(node).direction === "rtl";
/** @param {HTMLElement} pop */
const hide = (pop) => {
  try { if (matches(pop, ":popover-open")) pop.hidePopover(); } catch {}
};
/** Shows a popup, and has the overlay module place it in the same frame, so it is never seen where the browser first puts it. @param {HTMLElement} pop */
const show = (pop) => {
  try { pop.showPopover(); } catch { return; }
  if (has(pop, "data-ui-place")) T.dispatchEvent.call(pop, new Event("ui:place"));
};
/** The items of this menu alone, not of its submenus. @param {Element} menu @returns {HTMLElement[]} */
const itemsOf = (menu) => [...all(menu, ITEM)].filter((i) => closest(i, MENU) === menu && !(/** @type {HTMLInputElement} */ (i).disabled));
/** The menu a submenu hangs from, up to the first. @param {HTMLElement} menu */
const rootOf = (menu) => {
  let m = menu;
  while (has(m, "data-sub")) {
    const up = /** @type {HTMLElement | null} */ (closest(m.parentNode, MENU));
    if (!up) break;
    m = up;
  }
  return m;
};

// ---- Context menu --------------------------------------------------------

/** The context menu showing, and what takes focus back when it closes. @type {HTMLElement | null} */
let menu = null;
/** @type {HTMLElement | null} */
let back = null;
let lastKey = -1e9, press = 0, px = 0, py = 0;

function close() {
  if (menu) hide(menu);
}

/** @param {Element} region @param {number} x @param {number} y @param {boolean} keys */
function open(region, x, y, keys) {
  const pop = named(region, "data-ui-context");
  if (!pop || pop === menu) return;
  close();
  const now = /** @type {HTMLElement | null} */ (active());
  back = holds(region, now) ? now : has(region, "tabindex") ? /** @type {HTMLElement} */ (region) : null;
  show(pop);
  if (!matches(pop, ":popover-open")) return;
  menu = pop;
  // The popup is showing in this task, so its size is known and it is never seen elsewhere.
  const w = pop.offsetWidth, h = pop.offsetHeight, vw = innerWidth, vh = innerHeight;
  const left = rtl(pop) ? x - w : x;
  const top = y + h > vh - 8 ? y - h : y;
  pop.style.left = Math.max(8, Math.min(left, vw - w - 8)) + "px";
  pop.style.top = Math.max(8, Math.min(top, vh - h - 8)) + "px";
  if (keys) itemsOf(pop)[0]?.focus();
  else pop.focus({ preventScroll: true });
}

on(document, "contextmenu", (e) => {
  if (closest(e.target, "[data-ui-context-menu]")) return e.preventDefault();
  const region = closest(e.target, CTX);
  // The browser's own menu stays on a field, where it holds copy and paste.
  if (!region || e.defaultPrevented || closest(e.target, "input,textarea,select,[contenteditable]")) return;
  e.preventDefault();
  if (e.timeStamp - lastKey > 500) open(region, e.clientX, e.clientY, false);
});

on(document, "keydown", (e) => {
  const from = /** @type {HTMLElement | null} */ (active());
  if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
    const region = from && closest(from, CTX);
    if (!region || closest(from, "input,textarea,select,[contenteditable]")) return;
    e.preventDefault();
    lastKey = e.timeStamp;
    const box = /** @type {HTMLElement} */ (from).getBoundingClientRect();
    open(region, rtl(region) ? box.right : box.left, box.bottom, true);
  } else if (e.key === "Escape" && menu && holds(menu, from)) {
    // A submenu closes first, by itself; the menu is the next press.
    if ([...all(menu, "[data-sub]")].some((s) => matches(s, ":popover-open"))) return;
    e.preventDefault();
    close();
  }
});

on(document, "beforetoggle", (e) => {
  const pop = e.target;
  if (!pop.hasAttribute) return;
  const closing = e.newState !== "open";
  if (pop === menu && closing) {
    if (back && holds(pop, active())) back.focus({ preventScroll: true });
    menu = null;
  } else if (matches(pop, PANEL)) {
    const t = one(`[popovertarget="${CSS.escape(pop.id)}"]`);
    if (!t) return;
    set(t, "aria-expanded", String(!closing));
    if (closing && holds(pop, active())) /** @type {HTMLElement} */ (t).focus({ preventScroll: true });
  }
}, true);

// A press outside closes it; a long press on a touch screen opens it, as a right click does.
on(document, "pointerdown", (e) => {
  if (menu && !holds(menu, e.target)) close();
  clearTimeout(press);
  const region = e.pointerType === "mouse" ? null : closest(e.target, CTX);
  if (!region) return;
  px = e.clientX;
  py = e.clientY;
  press = window.setTimeout(() => open(region, px, py, false), 500);
}, true);
const lift = () => clearTimeout(press);
on(document, "pointerup", lift, true);
on(document, "pointercancel", lift, true);
on(document, "pointermove", (e) => {
  if (press && Math.hypot(e.clientX - px, e.clientY - py) > 10) lift();
}, { capture: true, passive: true });
on(document, "scroll", (e) => {
  lift();
  if (menu && !holds(menu, e.target)) close();
}, { capture: true, passive: true });
on(window, "resize", close);
on(window, "blur", close);

// ---- Menu bar ------------------------------------------------------------

/** @param {Element} bar @returns {HTMLElement[]} */
const buttons = (bar) => [...all(bar, TRIGGER)];

/** One tab stop in the bar: the button that has focus, or the first. @param {Element} bar @param {Element} [stop] */
function rove(bar, stop) {
  const list = buttons(bar);
  for (const b of list) b.tabIndex = b === (stop || list[0]) ? 0 : -1;
}

/** Opens the menu a button holds, with focus on its first or last item, or nowhere. @param {HTMLElement} button @param {string} [where] */
function drop(button, where) {
  const pop = named(button, "popovertarget");
  if (!pop) return;
  show(pop);
  const list = itemsOf(pop);
  (where === "last" ? list[list.length - 1] : where ? list[0] : null)?.focus();
}

/** Moves to the button `by` places on, or to the first or last, and opens its menu when one was open. @param {HTMLElement} bar @param {HTMLElement} from @param {number} by @param {boolean} opened */
function go(bar, from, by, opened) {
  const list = buttons(bar);
  const to = list[by === 0 ? 0 : by === 9 ? list.length - 1 : (list.indexOf(from) + by + list.length) % list.length];
  if (!to) return;
  if (opened) for (const b of list) hide(named(b, "popovertarget"));
  to.focus();
  rove(bar, to);
  if (opened) drop(to, "first");
}

on(document, "focusin", (e) => {
  const b = closest(e.target, TRIGGER);
  const bar = b && closest(b, BAR);
  if (bar) rove(bar, b);
});

on(document, "keydown", (e) => {
  if (e.ctrlKey || e.metaKey || e.altKey || e.defaultPrevented) return;
  const target = /** @type {HTMLElement | null} */ (closest(e.target, TRIGGER));
  const inMenu = !target && closest(e.target, MENU);
  const opener = inMenu && named(rootOf(/** @type {HTMLElement} */ (inMenu)), "aria-labelledby");
  const button = /** @type {HTMLElement | null} */ (target || (opener && closest(opener, TRIGGER)));
  const bar = button && /** @type {HTMLElement | null} */ (closest(button, BAR));
  if (!button || !bar) return;
  const next = rtl(bar) ? "ArrowLeft" : "ArrowRight", prev = rtl(bar) ? "ArrowRight" : "ArrowLeft";
  const by = e.key === next ? 1 : e.key === prev ? -1 : !inMenu && e.key === "Home" ? 0 : !inMenu && e.key === "End" ? 9 : null;
  if (by === null) return;
  e.preventDefault();
  go(bar, button, by, !!inMenu || attr(button, "aria-expanded") === "true");
});

// With one menu open, the pointer moving to another button opens that one.
on(document, "pointerover", (e) => {
  if (e.pointerType !== "mouse") return;
  const b = /** @type {HTMLElement | null} */ (closest(e.target, TRIGGER));
  const bar = b && closest(b, BAR);
  if (!b || !bar) return;
  const shown = buttons(bar).find((x) => x !== b && attr(x, "aria-expanded") === "true");
  if (!shown) return;
  b.focus({ preventScroll: true });
  rove(bar, b);
  if (has(b, "popovertarget")) drop(b);
  else hide(named(shown, "popovertarget"));
}, { passive: true });

// ---- Navigation menu -----------------------------------------------------

/** @param {Element} nav @returns {HTMLElement[]} */
const panels = (nav) => [...all(nav, PANEL)].filter((p) => matches(p, ":popover-open"));
let lingerTimer = 0, hoverTimer = 0, hoveredAt = 0;

on(document, "pointerover", (e) => {
  if (e.pointerType !== "mouse") return;
  const nav = closest(e.target, NAV);
  if (!nav) return;
  clearTimeout(lingerTimer);
  const t = closest(e.target, TOP);
  if (!t) return;
  clearTimeout(hoverTimer);
  const pop = named(t, "popovertarget");
  // Resting on a plain link closes the panel that is open.
  if (!pop) return panels(nav).forEach(hide);
  if (matches(pop, ":popover-open")) return;
  // A panel is open already: the next opens at once. Otherwise after a pause that a pointer passing over doesn't make.
  hoverTimer = window.setTimeout(() => {
    for (const p of panels(nav)) hide(p);
    show(pop);
    hoveredAt = e.timeStamp;
  }, panels(nav).length ? 0 : 120);
}, { passive: true });

on(document, "pointerout", (e) => {
  if (e.pointerType !== "mouse") return;
  const nav = closest(e.target, NAV);
  if (!nav || holds(nav, e.relatedTarget)) return;
  clearTimeout(hoverTimer);
  lingerTimer = window.setTimeout(() => { for (const p of panels(nav)) hide(p); }, 250);
}, { passive: true });

// A press on a button the pointer just opened keeps its panel open, instead of closing it.
on(document, "click", (e) => {
  const t = closest(e.target, TOP);
  const pop = t && closest(t, NAV) && named(t, "popovertarget");
  if (pop && e.timeStamp - hoveredAt < 700 && matches(pop, ":popover-open")) {
    hoveredAt = 0;
    e.preventDefault();
  }
}, true);

on(document, "keydown", (e) => {
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  const t = /** @type {HTMLElement | null} */ (closest(e.target, TOP));
  const nav = t && closest(t, NAV);
  if (!t || !nav) return;
  const list = /** @type {HTMLElement[]} */ ([...all(nav, TOP)]);
  const next = rtl(nav) ? "ArrowLeft" : "ArrowRight", prev = rtl(nav) ? "ArrowRight" : "ArrowLeft";
  const at = list.indexOf(t);
  const to = e.key === next ? list[at + 1] || list[0] : e.key === prev ? list[at - 1] || list[list.length - 1] : e.key === "Home" ? list[0] : e.key === "End" ? list[list.length - 1] : null;
  if (!to) return;
  e.preventDefault();
  for (const p of panels(nav)) hide(p);
  to.focus();
});

// Focus leaving a button and its panel for the rest of the page closes the panel.
on(document, "focusout", (e) => {
  const item = closest(e.target, "[data-slot=navigation-menu-item]");
  if (!item || !e.relatedTarget || holds(item, e.relatedTarget)) return;
  for (const p of all(item, PANEL)) hide(/** @type {HTMLElement} */ (p));
});

// ---- Start ---------------------------------------------------------------

function enhance() {
  for (const bar of all(root(), BAR)) rove(bar);
  for (const t of all(root(), NAV + " button[popovertarget]")) if (!has(t, "aria-expanded")) set(t, "aria-expanded", "false");
}
enhance();
on(document, "ui:swap", enhance);
