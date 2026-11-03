// Tabs, toggle groups and dropdown menus: what only a browser can do. The markup
// already works with no script (a tab is a link, a toggle is a radio or a checkbox,
// a menu is a popover), so this adds the keys of the patterns, switching in the
// page, the address bar, submenus, typeahead and closing after a choice. It is
// delegated from `document`, so a morph needs no wiring, and it reads every
// element through the prototypes, so a name in stored markup can't shadow a method.

const E = Element.prototype;
/** @param {any} node @param {string} selector @returns {Element | null} */
const closest = (node, selector) => (node instanceof Element ? E.closest.call(node, selector) : null);
/** @param {ParentNode} node @param {string} selector @returns {HTMLElement[]} */
const all = (node, selector) => [.../** @type {any} */ (E.querySelectorAll.call(node, selector))];
/** @param {Element} node @param {string} selector @returns {HTMLElement | null} */
const one = (node, selector) => /** @type {any} */ (E.querySelector.call(node, selector));
/** @param {Element} node @param {string} name */
const get = (node, name) => E.getAttribute.call(node, name);
/** @param {Element} node @param {string} name */
const has = (node, name) => E.hasAttribute.call(node, name);
/** @param {Element} node @param {string} name @param {string} value */
const set = (node, name, value) => E.setAttribute.call(node, name, value);
/** @param {Element} node @param {string} name */
const unset = (node, name) => E.removeAttribute.call(node, name);
/** @param {EventTarget} target @param {string} type @param {(e: any) => void} listener @param {boolean | AddEventListenerOptions} [options] */
const on = (target, type, listener, options) => EventTarget.prototype.addEventListener.call(target, type, listener, options);
/** @param {string | null} id @returns {HTMLElement | null} */
const byId = (id) => (id ? Document.prototype.getElementById.call(document, id) : null);
/** The document's own members, through the prototypes: a form named like one can't stand in for it. */
const D = Document.prototype;
const gRoot = /** @type {() => HTMLElement} */ (Object.getOwnPropertyDescriptor(D, "documentElement")?.get);
const gActive = /** @type {() => Element | null} */ (Object.getOwnPropertyDescriptor(D, "activeElement")?.get);
/** The root element. */
const docEl = () => gRoot.call(document);
/** The element in focus. */
const active = () => gActive.call(document);
/** @param {string} tag @returns {HTMLElement} */
const make = (tag) => D.createElement.call(document, tag);
/** @param {EventTarget} target @param {Event} event */
const fire = (target, event) => EventTarget.prototype.dispatchEvent.call(target, event);
/** Whether `a` holds `b`. @param {Node | null} a @param {unknown} b */
const holds = (a, b) => !!a && b instanceof Node && Node.prototype.contains.call(a, b);
/** @param {Element} node */
const rtl = (node) => getComputedStyle(node).direction === "rtl";
/** @param {Element} node @param {string} state */
const matches = (node, state) => E.matches.call(node, state);

/** Whether a click is a plain left click, which the page may take over. @param {MouseEvent} e */
const plain = (e) => e.button === 0 && !e.metaKey && !e.ctrlKey && !e.shiftKey && !e.altKey;

/**
 * Which way the arrow keys go: `next` and `prev` are key names.
 * @param {Element} list @param {boolean} vertical
 */
function arrows(list, vertical) {
  const flip = !vertical && rtl(list);
  return vertical ? { next: "ArrowDown", prev: "ArrowUp" } : flip ? { next: "ArrowLeft", prev: "ArrowRight" } : { next: "ArrowRight", prev: "ArrowLeft" };
}

/**
 * The item a key goes to, or null for a key that doesn't move.
 * @param {HTMLElement[]} items @param {number} at @param {string} key @param {{next: string, prev: string}} keys
 */
function target(items, at, key, keys) {
  if (!items.length) return null;
  if (key === "Home") return items[0];
  if (key === "End") return items[items.length - 1];
  if (key === keys.next) return items[(at + 1) % items.length];
  if (key === keys.prev) return items[(at <= 0 ? items.length : at) - 1];
  return null;
}

// ---- Tabs ----------------------------------------------------------------

/** @param {Element} root */
const tablist = (root) => /** @type {HTMLElement} */ (one(root, "[role=tablist]"));
/** @param {Element} root */
const tabsIn = (root) => all(tablist(root), "[role=tab]");
/** @param {Element} tab */
const enabled = (tab) => get(tab, "aria-disabled") !== "true";
/** @param {Element} tab */
const panelOf = (tab) => byId(get(tab, "aria-controls"));

/** One tab stop in a list: the open tab, or the one that has focus. @param {Element} root @param {HTMLElement} [focused] */
function rove(root, focused) {
  const tabs = tabsIn(root);
  const open = tabs.find((t) => get(t, "aria-selected") === "true" && enabled(t)) || tabs.find(enabled);
  for (const tab of tabs) tab.tabIndex = tab === (focused || open) ? 0 : -1;
}

/** @param {HTMLElement} panel */
function load(panel) {
  set(panel, "data-ui-loaded", "");
  set(panel, "aria-busy", "true");
  fire(panel, new CustomEvent("ui:refresh", { bubbles: true }));
}

/** Puts the open tab in the address. @param {Element} root @param {Element} tab @param {string} mode */
function writeUrl(root, tab, mode) {
  const param = get(root, "data-ui-tabs") || "tab";
  const key = get(tab, "data-key") || "";
  const url = new URL(location.href);
  if (url.searchParams.get(param) === key) return;
  url.searchParams.set(param, key);
  if (mode === "push") history.pushState(null, "", url);
  else history.replaceState(null, "", url);
  here = pageKey();
}

/**
 * Opens `tab`. `url` says how the address follows: "push", "replace" or nothing.
 * @param {Element} root @param {HTMLElement} tab @param {string} [url]
 */
function select(root, tab, url) {
  if (!enabled(tab)) return;
  const findable = has(root, "data-findable");
  for (const t of tabsIn(root)) {
    const on = t === tab;
    set(t, "aria-selected", String(on));
    if (on) set(t, "data-active", "");
    else unset(t, "data-active");
    const panel = panelOf(t);
    if (!panel) continue;
    if (on) {
      unset(panel, "hidden");
      set(panel, "data-active", "");
    } else {
      set(panel, "hidden", findable && !has(panel, "data-ui-src") ? "until-found" : "");
      unset(panel, "data-active");
    }
  }
  rove(root);
  const panel = panelOf(tab);
  if (panel && has(panel, "data-ui-src") && (has(panel, "data-ui-fresh") || !has(panel, "data-ui-loaded"))) load(panel);
  tab.scrollIntoView({ block: "nearest", inline: "nearest" });
  fade(tablist(root));
  if (url) writeUrl(root, tab, url);
}

/** The page without its tabs' parameters: when it stays the same, back and forward only switch tabs. */
function pageKey() {
  const url = new URL(location.href);
  const mine = new Set(all(docEl(), "[data-ui-tabs]").map((r) => get(r, "data-ui-tabs")));
  const rest = [...url.searchParams].filter(([k]) => !mine.has(k)).map(([k, v]) => k + "=" + v).sort();
  return url.pathname + "?" + rest.join("&");
}
let here = pageKey();

on(document, "click", (e) => {
  const tab = closest(e.target, "[role=tab]");
  const root = tab && closest(tab, "[data-ui-tabs]");
  if (!tab || !root || e.defaultPrevented || !plain(e)) return;
  e.preventDefault();
  select(root, /** @type {HTMLElement} */ (tab), "push");
});

on(document, "keydown", (e) => {
  const tab = /** @type {HTMLElement | null} */ (closest(e.target, "[role=tab]"));
  const root = tab && closest(tab, "[data-ui-tabs]");
  if (!tab || !root || e.ctrlKey || e.metaKey || e.altKey) return;
  if (e.key === "Enter" || e.key === " ") {
    e.preventDefault();
    select(root, tab, "push");
    return;
  }
  const list = tabsIn(root).filter(enabled);
  const to = target(list, list.indexOf(tab), e.key, arrows(tablist(root), get(root, "data-orientation") === "vertical"));
  if (!to) return;
  e.preventDefault();
  if (get(root, "data-activation") === "manual") rove(root, to);
  else select(root, to, "replace");
  to.focus();
});

// A manual list keeps its tab stop on the open tab once focus leaves it.
on(document, "focusout", (e) => {
  const tab = closest(e.target, "[role=tab]");
  const root = tab && closest(tab, "[data-ui-tabs]");
  if (root && !holds(tablist(root), e.relatedTarget)) rove(root);
});

// Back and forward within one page's tabs need no request: the entry is ours when only the
// tabs' parameters differ from the page that is showing.
function claim() {
  const now = pageKey();
  const roots = all(docEl(), "[data-ui-tabs]");
  if (now !== here || !roots.length) {
    here = now;
    return false;
  }
  const url = new URL(location.href);
  for (const root of roots) {
    const key = url.searchParams.get(get(root, "data-ui-tabs") || "tab");
    const tabs = tabsIn(root);
    const tab = tabs.find((t) => get(t, "data-key") === key && enabled(t)) || tabs.find(enabled);
    if (tab) select(root, tab);
  }
  return true;
}
// With the core runtime, the page's own handler asks first; without it nothing else would answer.
on(document, "ui:popstate", (e) => { if (claim()) e.preventDefault(); });
on(window, "popstate", () => { if (!has(docEl(), "data-build")) claim(); });

// Find-in-page matched text in a panel that is not showing.
on(document, "beforematch", (e) => {
  for (let panel = closest(e.target, "[role=tabpanel]"); panel; panel = closest(panel.parentNode, "[role=tabpanel]")) {
    const tab = byId(get(panel, "aria-labelledby"));
    const root = closest(panel, "[data-ui-tabs]");
    if (tab && root) select(root, tab, "replace");
  }
});

// A panel that could not be fetched says so, and tries again on Retry.
/** @param {Element} panel */
function failedPanel(panel) {
  const root = closest(panel, "[data-ui-tabs]");
  if (!root) return;
  unset(panel, "data-ui-loaded");
  unset(panel, "aria-busy");
  let words = /** @type {{failed?: string, retry?: string}} */ ({});
  try { words = JSON.parse(get(root, "data-l") || "{}"); } catch {}
  for (const old of all(panel, "[data-slot=tab-alert]")) old.remove();
  const alert = make("div");
  set(alert, "data-slot", "tab-alert");
  set(alert, "role", "alert");
  const text = make("p");
  text.textContent = words.failed || "This tab didn't load.";
  const retry = make("button");
  set(retry, "type", "button");
  set(retry, "data-slot", "button");
  set(retry, "data-variant", "outline");
  set(retry, "data-size", "sm");
  set(retry, "data-ui-refresh", "");
  retry.textContent = words.retry || "Retry";
  alert.append(text, retry);
  if (one(panel, "[data-slot=skeleton]")) panel.replaceChildren(alert);
  else panel.prepend(alert);
}
on(document, "ui:region-error", (e) => {
  const panel = closest(e.target, "[role=tabpanel][data-ui-src]");
  if (panel) failedPanel(panel);
});
// A panel that failed before this module arrived: the event is gone, the core runtime's mark stays.
for (const panel of all(docEl(), "[role=tabpanel][data-ui-src][data-ui-failed]")) failedPanel(panel);
on(document, "click", (e) => {
  const retry = closest(e.target, "[data-slot=tab-alert] [data-ui-refresh]");
  const panel = retry && closest(retry, "[role=tabpanel]");
  if (panel) {
    set(panel, "data-ui-loaded", "");
    set(panel, "aria-busy", "true");
  }
});

// ---- Toggle groups -------------------------------------------------------

/** @param {Element} group */
const toggles = (group) => /** @type {HTMLInputElement[]} */ (all(group, "input").filter((i) => !(/** @type {HTMLInputElement} */ (i).disabled)));

/** One tab stop in a group of checkboxes; radios already have one. @param {Element} group @param {HTMLInputElement} [focused] */
function roveGroup(group, focused) {
  const inputs = toggles(group);
  const stop = focused || inputs.find((i) => i.checked) || inputs[0];
  for (const input of inputs) input.tabIndex = input === stop ? 0 : -1;
}

/** @param {Element} group */
const remember = (group) => {
  const on = all(group, "input").find((i) => /** @type {HTMLInputElement} */ (i).checked);
  set(group, "data-ui-value", on ? /** @type {HTMLInputElement} */ (on).value : "");
};

on(document, "focusin", (e) => {
  const input = closest(e.target, "[data-ui-toggle-group] input");
  const group = input && closest(input, "[data-ui-toggle-group]");
  if (group && get(group, "role") === "group") roveGroup(group, /** @type {HTMLInputElement} */ (input));
});

on(document, "keydown", (e) => {
  const input = /** @type {HTMLInputElement | null} */ (closest(e.target, "[data-ui-toggle-group] input"));
  const group = input && closest(input, "[data-ui-toggle-group]");
  if (!input || !group || e.ctrlKey || e.metaKey || e.altKey) return;
  if (e.key === "Enter" && get(group, "role") === "group") {
    e.preventDefault();
    input.click();
    return;
  }
  // Radios have their own arrows, and only Home and End are missing.
  const keys = get(group, "role") === "radiogroup" && e.key !== "Home" && e.key !== "End" ? { next: "", prev: "" } : arrows(group, get(group, "data-orientation") === "vertical");
  const list = toggles(group);
  const to = target(list, list.indexOf(input), e.key, keys);
  if (!to) return;
  e.preventDefault();
  to.focus();
  if (get(group, "role") === "radiogroup") to.click();
});

on(document, "click", (e) => {
  const input = /** @type {HTMLInputElement | null} */ (closest(e.target, "[data-ui-deselect] input[type=radio]"));
  const group = input && closest(input, "[data-ui-deselect]");
  if (!input || !group) return;
  // The radio is already on when the click arrives: the group remembers what was on before.
  if (get(group, "data-ui-value") === input.value) {
    input.checked = false;
    set(group, "data-ui-value", "");
    fire(input, new Event("input", { bubbles: true }));
    fire(input, new Event("change", { bubbles: true }));
  }
});
on(document, "change", (e) => {
  const group = closest(e.target, "[data-ui-deselect]");
  if (group) remember(group);
});

// ---- Dropdown menus ------------------------------------------------------

const ITEM = "[role=menuitem],[role=menuitemcheckbox],[role=menuitemradio]";

/** The items of this menu alone, not of its submenus. @param {Element} menu */
const itemsOf = (menu) => all(menu, ITEM).filter((i) => closest(i, "[data-ui-menu]") === menu);
/** The items the arrows reach: disabled ones are, native-disabled inputs can't be. @param {Element} menu */
const reachable = (menu) => itemsOf(menu).filter((i) => !(/** @type {HTMLInputElement} */ (i).disabled));
/** The menu a submenu hangs from, up to the first. @param {HTMLElement} menu */
const rootOf = (menu) => {
  let m = menu;
  while (has(m, "data-sub")) {
    const up = /** @type {HTMLElement | null} */ (closest(m.parentNode, "[data-ui-menu]"));
    if (!up) break;
    m = up;
  }
  return m;
};
/** @param {Element} menu */
const isOpen = (menu) => matches(menu, ":popover-open");
/** @param {HTMLElement} menu */
const hide = (menu) => {
  try { if (isOpen(menu)) menu.hidePopover(); } catch {}
};
/** @param {Element} item */
const isDisabled = (item) => get(item, "aria-disabled") === "true" || /** @type {HTMLInputElement} */ (item).disabled === true;
/** @param {Element} item */
const isSub = (item) => has(item, "data-sub-trigger");
/** The words an item starts with, for typeahead. @param {Element} item */
const textOf = (item) => {
  const box = closest(item, "[data-slot=menu-item]") || item;
  return (one(box, "[data-slot=menu-text]") || box).textContent?.trim().toLowerCase() || "";
};

/** Where focus goes when the menu that is opening opens: "first", "last", "menu", or nowhere. */
let want = "";

/** The submenu a menu item opens. @param {Element} trigger */
function subOf(trigger) {
  return byId(get(trigger, "popovertarget"));
}

/** @param {HTMLElement} trigger @param {boolean} focusIn */
function openSub(trigger, focusIn) {
  const sub = subOf(trigger);
  if (!sub || isDisabled(trigger)) return;
  if (!isOpen(sub)) {
    want = "";
    try { sub.showPopover(); } catch {}
    if (!isOpen(sub)) return;
  }
  // The menu is showing as soon as showPopover returns, so keys that follow find focus inside.
  if (focusIn) reachable(sub)[0]?.focus();
}

// A menu button opens with a press, and with the arrows from the keyboard.
on(document, "click", (e) => {
  const trigger = closest(e.target, "[aria-haspopup=menu][popovertarget]");
  if (trigger) want = closest(trigger, "[data-ui-menu]") ? (e.detail === 0 ? "first" : "") : e.detail === 0 ? "first" : "menu";
}, true);

// The state a button reports changes in the same task as the menu does, and focus goes
// back to the item or button that opened a menu it was inside, whichever way it closes
// (browsers differ on doing that themselves, and not at all for a nested one).
on(document, "beforetoggle", (e) => {
  const menu = e.target;
  const trigger = menu.hasAttribute?.("data-ui-menu") && byId(get(menu, "aria-labelledby"));
  if (!trigger) return;
  set(trigger, "aria-expanded", String(e.newState === "open"));
  if (e.newState !== "open" && holds(menu, active())) trigger.focus({ preventScroll: true });
}, true);

on(document, "toggle", (e) => {
  const menu = /** @type {HTMLElement} */ (e.target);
  if (!menu.hasAttribute?.("data-ui-menu")) return;
  const opening = e.newState === "open";
  buffer = "";
  if (!opening) {
    want = "";
    hovered = null;
    return;
  }
  for (const item of itemsOf(menu)) item.tabIndex = -1;
  const items = reachable(menu);
  const to = want === "first" ? items[0] : want === "last" ? items[items.length - 1] : want === "menu" ? menu : null;
  want = "";
  to?.focus({ preventScroll: true });
}, true);

// Typeahead: letters typed together pick the next item that starts with them.
let buffer = "";
let bufferTimer = 0;
/** @param {HTMLElement} menu @param {string} key @param {Element | null} current */
function typeahead(menu, key, current) {
  clearTimeout(bufferTimer);
  bufferTimer = window.setTimeout(() => { buffer = ""; }, 500);
  buffer += key.toLowerCase();
  const items = reachable(menu);
  const repeated = [...buffer].every((c) => c === buffer[0]);
  const query = repeated ? buffer[0] : buffer;
  const start = items.indexOf(/** @type {HTMLElement} */ (current)) + (repeated ? 1 : 0);
  for (let k = 0; k < items.length; k++) {
    const item = items[(Math.max(start, 0) + k) % items.length];
    if (textOf(item).startsWith(query)) {
      item.focus();
      return;
    }
  }
}

on(document, "keydown", (e) => {
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  // On a menu button outside a menu: the arrows open it.
  const button = /** @type {HTMLElement | null} */ (closest(e.target, "[aria-haspopup=menu][popovertarget]"));
  if (button && !closest(button, "[data-ui-menu]")) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      const menu = byId(get(button, "popovertarget"));
      if (!menu) return;
      e.preventDefault();
      if (!isOpen(menu)) {
        want = "";
        try { menu.showPopover(); } catch {}
      }
      const items = reachable(menu);
      (e.key === "ArrowUp" ? items[items.length - 1] : items[0])?.focus();
    }
    return;
  }
  const menu = /** @type {HTMLElement | null} */ (closest(e.target, "[data-ui-menu]"));
  if (!menu) return;
  const current = closest(e.target, ITEM);
  const here = current && closest(current, "[data-ui-menu]") === menu ? /** @type {HTMLElement} */ (current) : null;
  const items = reachable(menu);
  const keys = arrows(menu, true);
  const sideways = rtl(menu) ? { open: "ArrowLeft", close: "ArrowRight" } : { open: "ArrowRight", close: "ArrowLeft" };
  if (e.key === "Tab") {
    hide(rootOf(menu));
    return;
  }
  if (e.key === sideways.open && here && isSub(here)) {
    e.preventDefault();
    openSub(here, true);
    return;
  }
  if (e.key === sideways.close && has(menu, "data-sub")) {
    e.preventDefault();
    const opener = byId(get(menu, "aria-labelledby"));
    hide(menu);
    opener?.focus();
    return;
  }
  if (e.key === "Enter" || (e.key === " " && !buffer)) {
    if (!here) {
      if (e.key === " ") e.preventDefault();
      return;
    }
    if (isSub(here)) {
      e.preventDefault();
      openSub(here, true);
    } else if (/** @type {HTMLElement} */ (here).nodeName === "INPUT") {
      // A space already toggles an input; Enter does not.
      if (e.key === "Enter") {
        e.preventDefault();
        here.click();
      }
    } else {
      e.preventDefault();
      if (!isDisabled(here)) here.click();
    }
    return;
  }
  const to = target(items, here ? items.indexOf(here) : -1, e.key, keys);
  if (to) {
    e.preventDefault();
    to.focus();
    return;
  }
  if (e.key.length === 1) {
    e.preventDefault();
    typeahead(menu, e.key, here);
  }
});

// Choosing an item closes the menu, unless the item or the menu asks to stay open.
on(document, "click", (e) => {
  const item = closest(e.target, "[data-ui-menu] [data-slot=menu-item]");
  const menu = /** @type {HTMLElement | null} */ (item && closest(item, "[data-ui-menu]"));
  if (!item || !menu || isSub(item) || has(item, "data-disabled") || matches(item, ":has(input:disabled)") || has(item, "data-keep-open") || has(menu, "data-keep-open")) return;
  hide(rootOf(menu));
});

// The pointer lights the item under it, opens a submenu it rests on, and closes one
// it has left, after a pause long enough to cross to it.
/** @type {HTMLElement | null} */
let hovered = null;
let hoverTimer = 0;
on(document, "pointermove", (e) => {
  if (e.pointerType !== "mouse") return;
  const box = closest(e.target, "[data-ui-menu] [data-slot=menu-item]");
  const inside = closest(e.target, "[data-ui-menu]");
  if (!box || !inside) {
    // Over a submenu's padding, or off the menus: leave things be.
    return;
  }
  const item = /** @type {HTMLElement | null} */ (has(box, "role") ? box : one(box, ITEM));
  if (!item || item === hovered) return;
  hovered = item;
  clearTimeout(hoverTimer);
  const menu = /** @type {HTMLElement} */ (closest(item, "[data-ui-menu]"));
  if (!(/** @type {HTMLInputElement} */ (item).disabled)) item.focus({ preventScroll: true });
  const open = all(menu, "[data-sub]").filter((s) => closest(s.parentNode, "[data-ui-menu]") === menu && isOpen(s));
  if (isSub(item)) {
    hoverTimer = window.setTimeout(() => {
      for (const sub of open) if (sub !== subOf(item)) hide(sub);
      if (hovered === item) openSub(item, false);
    }, 90);
  } else if (open.length) {
    hoverTimer = window.setTimeout(() => { for (const sub of open) hide(sub); }, 260);
  }
}, { passive: true });
// Reaching a submenu keeps it.
on(document, "pointerover", (e) => {
  if (closest(e.target, "[data-sub]")) clearTimeout(hoverTimer);
}, { passive: true });

// A list wider than its box fades where more tabs wait, since a phone hides its scrollbar.
/** @param {HTMLElement} list */
function fade(list) {
  const max = list.scrollWidth - list.clientWidth;
  const x = Math.abs(list.scrollLeft);
  const flip = rtl(list);
  list.toggleAttribute("data-fade-l", max > 1 && (flip ? x < max - 1 : x > 1));
  list.toggleAttribute("data-fade-r", max > 1 && (flip ? x > 1 : x < max - 1));
}
on(document, "scroll", (e) => { if (closest(e.target, "[role=tablist]")) fade(/** @type {HTMLElement} */ (e.target)); }, { capture: true, passive: true });
on(window, "resize", () => { for (const list of all(docEl(), "[role=tablist]")) fade(list); });
on(window, "load", () => { for (const list of all(docEl(), "[role=tablist]")) fade(list); });

// ---- Start ---------------------------------------------------------------

function enhance() {
  for (const root of all(docEl(), "[data-ui-tabs]")) {
    // Markup that claims to be tabs and has no list is left alone.
    if (!tablist(root)) continue;
    rove(root);
    fade(tablist(root));
  }
  for (const group of all(docEl(), "[data-ui-toggle-group]")) {
    if (get(group, "role") === "group") roveGroup(group);
    if (has(group, "data-ui-deselect")) remember(group);
  }
  here = pageKey();
}
enhance();
on(document, "ui:swap", enhance);
