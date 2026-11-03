// What ops and commands share: opening things, toasts, the announcer.
//
// A toast lives in a CSS animation (`ui-toast-life`, as long as its `data-duration`)
// and ends on `animationend`; pausing under the pointer or focus is
// `animation-play-state`, so there is no timer to lose and nothing that can stick.
import { active, attr, body, closest, cookies, every, has, hidden, holds, keeping, make, on, one, persistent, root, set } from "./dom.js";

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
    live = make("div");
    live.setAttribute("data-slot", "announcer");
    live.setAttribute("aria-live", "polite");
    persistent.add(live);
    body().append(live);
  }
  live.textContent = text;
}

/** @typedef {{tone?: string, text?: string, title?: string, id?: string, duration?: string, closable?: boolean, action?: {label?: string, href?: string, post?: boolean}}} ToastSpec */

const TONES = ["info", "success", "warning", "danger"];
const DURATIONS = ["brief", "normal", "long", "sticky"];
const KEY = "navree-ui-toasts";
const LIFE = "ui-toast-life";
/** How long a toast waits in the tab for the next page: the flash cookie's life. */
const KEPT_MS = 30000;

/** @type {HTMLElement | undefined} */
let host;
/** Where focus was before a hotkey took it into the toasts. @type {Element | null} */
let returnTo = null;
/** The user the toasts on show were raised for: a short hash of the session's token. @type {string | null} */
let user = null;
let serial = 0;

/**
 * Who is signed in, as far as a script can tell: the session's token cookie, as a
 * short hash so the token itself is never kept. No token, no session: nothing to tell apart.
 */
function who() {
  let token = "";
  for (const part of cookies().split("; ")) if (part.startsWith("XSRF-TOKEN=")) token = part.slice(11);
  let hash = 5381;
  for (let i = 0; i < token.length; i++) hash = ((hash * 33) ^ token.charCodeAt(i)) >>> 0;
  return token ? hash.toString(36) : "";
}

/**
 * Whether the user is the one the toasts on show were raised for. If another signed in
 * or out, those toasts and the posts they offer go: they are not this user's.
 */
export function sameUser() {
  const now = who();
  if (user === null) user = now;
  if (now === user) return true;
  user = now;
  for (const t of host?.querySelectorAll("[data-slot=toast]") || []) t.remove();
  try { host?.hidePopover(); } catch {}
  return false;
}

/** The toasts' one standing place: the page's own, or one made now. It stays put through every navigation. */
function toaster() {
  if (host?.isConnected) return host;
  // Only the page's own, a direct child of the body: stored markup can't stand in for it.
  host = /** @type {HTMLElement | undefined} */ ([...body().children].find((el) => attr(el, "data-slot") === "toaster"));
  if (!host) {
    host = make("section");
    host.setAttribute("data-slot", "toaster");
    host.setAttribute("aria-label", "Notifications");
    host.setAttribute("data-position", "bottom-end");
    host.setAttribute("data-close", "Dismiss");
  }
  if (!has(host, "id")) host.setAttribute("id", "ui-toaster");
  if (!host.parentNode) body().append(host);
  persistent.add(host);
  keeping.add(host);
  // A popover is in the top layer, so a toast shows above an open dialog. Without the
  // runtime the same element is a fixed box.
  if (!host.hasAttribute("popover")) host.setAttribute("popover", "manual");
  return host;
}

/** A URL a toast may offer: a link to the web, or a post to this site. @param {unknown} href @param {boolean} post */
function safe(href, post) {
  if (typeof href !== "string") return "";
  try {
    const url = new URL(href, location.href);
    return (post ? url.origin === location.origin : /^(https?|mailto|tel):$/.test(url.protocol)) ? href : "";
  } catch {
    return "";
  }
}

/** The element for `spec`: the same markup the server draws for a `Toast`. @param {ToastSpec} spec */
function build(spec) {
  const tone = TONES.includes(/** @type {string} */ (spec.tone)) ? /** @type {string} */ (spec.tone) : "info";
  const action = spec.action && typeof spec.action.label === "string" ? spec.action : undefined;
  const href = action ? safe(action.href, !!action.post) : "";
  const duration = DURATIONS.includes(/** @type {string} */ (spec.duration))
    ? /** @type {string} */ (spec.duration)
    : (action && href) || tone === "warning" || tone === "danger" ? "long" : "normal";
  const el = make("div");
  el.setAttribute("data-slot", "toast");
  el.setAttribute("data-tone", tone);
  el.setAttribute("data-duration", duration);
  el.setAttribute("role", tone === "danger" ? "alert" : "status");
  if (typeof spec.id === "string" && spec.id) el.setAttribute("data-toast-id", spec.id);
  const text = make("div");
  text.setAttribute("data-slot", "toast-body");
  if (spec.title) {
    const title = make("p");
    title.setAttribute("data-slot", "toast-title");
    title.textContent = String(spec.title);
    text.append(title);
  }
  const line = make("p");
  line.setAttribute("data-slot", "toast-text");
  line.textContent = String(spec.text ?? "");
  text.append(line);
  el.append(text);
  if (action && href) {
    const label = String(action.label);
    if (action.post) {
      const form = make("form");
      form.setAttribute("method", "post");
      form.setAttribute("action", href);
      form.setAttribute("data-ui-form", "");
      const button = make("button");
      button.setAttribute("data-slot", "toast-action");
      button.setAttribute("type", "submit");
      button.textContent = label;
      form.append(button);
      el.append(form);
    } else {
      const link = make("a");
      link.setAttribute("data-slot", "toast-action");
      link.setAttribute("href", href);
      link.textContent = label;
      el.append(link);
    }
  }
  // A toast that stays has to be able to go: the close button is not optional for a sticky one.
  if (spec.closable !== false || duration === "sticky") {
    const close = make("a");
    close.setAttribute("data-slot", "toast-close");
    close.setAttribute("href", "#");
    close.setAttribute("aria-label", attr(toaster(), "data-close") || "Dismiss");
    el.append(close);
  }
  return el;
}

/**
 * Gives a toast an address of its own, and its close link the same. With no script the
 * link goes to the address and the page hides the toast that is the target; here the
 * click is the runtime's, and a fresh address keeps toasts taken in from other pages from clashing.
 * @param {Element} el
 */
function name(el) {
  const id = "ui-toast-" + ++serial;
  set(el, "id", id);
  const close = el.querySelector("[data-slot=toast-close]");
  if (close) set(close, "href", "#" + id);
}

/** The animation that is a toast's time, if it has one. @param {Element} t */
const lifeOf = (t) => t.getAnimations().find((a) => /** @type {CSSAnimation} */ (a).animationName === LIFE);

/** Puts `el` in the standing toaster, in place of a toast with its name. @param {HTMLElement} el @param {number} [elapsed] */
function place(el, elapsed) {
  sameUser();
  const box = toaster();
  const id = attr(el, "data-toast-id");
  const old = id && [...box.children].find((t) => attr(t, "data-toast-id") === id);
  name(el);
  if (old) old.replaceWith(el);
  else box.append(el);
  reseat();
  if (elapsed) {
    const life = lifeOf(el);
    if (life) life.currentTime = elapsed;
  }
}

/**
 * The dialogs open as modals, in the order they opened: the last is on top. The page
 * outside the top one is inert, so that is where the toaster has to stand to be used.
 * @type {HTMLDialogElement[]}
 */
let modals = [];
/** @type {MutationObserver | undefined} */
let watcher;

/** @param {MutationRecord[]} records */
function noted(records) {
  for (const record of records) {
    const dialog = record.target;
    if (!(dialog instanceof HTMLDialogElement)) continue;
    modals = modals.filter((d) => d !== dialog);
    if (has(dialog, "open") && dialog.matches(":modal")) modals.push(dialog);
  }
  modals = modals.filter((d) => d.isConnected && d.matches(":modal"));
}

/** The modal dialog on top, if one is open. */
function topModal() {
  if (watcher) noted(watcher.takeRecords());
  return /** @type {HTMLDialogElement | null} */ (modals[modals.length - 1] || one("dialog:modal"));
}

/**
 * Sets the toaster where it can be used and shows it if it has toasts: in the top
 * modal dialog while one is open, otherwise in the page. Moving it restarts what is
 * animated in it, so each toast's time carries across.
 */
function reseat() {
  if (!host?.isConnected) return;
  const toasts = [...host.querySelectorAll("[data-slot=toast]")];
  let want;
  if (toasts.length) want = topModal() || body();
  else {
    // With nothing to show it stays where it is while that is a modal still open, and goes home otherwise.
    const inside = closest(host, "dialog");
    want = inside && inside.matches(":modal") ? inside : body();
  }
  try {
    if (host.parentNode !== want) {
      const times = toasts.map((t) => Number(lifeOf(t)?.currentTime) || 0);
      want.append(host);
      toasts.forEach((t, i) => {
        const life = times[i] ? lifeOf(t) : undefined;
        if (life) life.currentTime = times[i];
      });
    }
    if (toasts.length && !host.matches(":popover-open")) host.showPopover();
  } catch {}
}

/** Takes a toast away, and focus with it to where it came from. @param {Element} t */
function dismiss(t) {
  const held = holds(t, active());
  t.remove();
  if (held && returnTo?.isConnected) /** @type {HTMLElement} */ (returnTo).focus({ preventScroll: true });
  if (host && !host.querySelector("[data-slot=toast]")) {
    try { host.hidePopover(); } catch {}
  }
}

/**
 * Shows a toast. `toast("danger", "Text")`, or a `ToastSpec` as the server sends it.
 * @param {string | ToastSpec} tone @param {string} [text]
 */
export function toast(tone, text) {
  place(build(typeof tone === "object" && tone ? tone : { tone, text }));
}

/**
 * Takes the toasts of a fetched page into the standing toaster, so a toast left by
 * the last request shows and the toaster itself is never morphed away.
 * @param {Document} doc
 */
export function adoptToasts(doc) {
  const from = [...body(doc).children].find((el) => attr(el, "data-slot") === "toaster");
  if (!from) return;
  sameUser();
  const box = toaster();
  // What the page says about the region itself: where it stands, and its words, in the page's language.
  for (const field of ["data-position", "data-close", "aria-label"]) {
    const value = attr(from, field);
    if (value) box.setAttribute(field, value);
  }
  const kids = [...from.children].filter((t) => attr(t, "data-slot") === "toast");
  from.remove();
  for (const kid of kids) place(/** @type {HTMLElement} */ (kid));
}

/** What a showing toast says, to show again on the next page. @param {Element} t */
function specOf(t) {
  const text = (/** @type {string} */ s) => t.querySelector(s)?.textContent || undefined;
  const link = t.querySelector("a[data-slot=toast-action]");
  const form = t.querySelector("form");
  const action = link
    ? { label: link.textContent || "", href: attr(link, "href") || "" }
    : form ? { label: form.textContent || "", href: attr(form, "action") || "", post: true } : undefined;
  return {
    tone: attr(t, "data-tone"),
    text: text("[data-slot=toast-text]"),
    title: text("[data-slot=toast-title]"),
    id: attr(t, "data-toast-id") || undefined,
    duration: attr(t, "data-duration"),
    closable: !!t.querySelector("[data-slot=toast-close]"),
    action,
  };
}

/**
 * Writes the toasts still showing, with the time they have left, for the page that comes
 * next: for whom they were raised and when, so they go stale like a flash does.
 */
function handOff() {
  if (!host?.isConnected) return;
  sameUser();
  const list = [];
  for (const t of host.querySelectorAll("[data-slot=toast]")) {
    list.push({ ...specOf(t), elapsed: Number(lifeOf(t)?.currentTime) || 0 });
  }
  try {
    if (list.length) sessionStorage.setItem(KEY, JSON.stringify({ at: Date.now(), who: user, toasts: list }));
  } catch {}
}

/** Starts the toasts: marks the page's own toaster, shows what the last page left, and listens. */
export function startToasts() {
  const box = toaster();
  user = who();
  for (const t of box.querySelectorAll("[data-slot=toast]")) name(t);
  /** @type {any} */
  let left = null;
  try {
    left = JSON.parse(sessionStorage.getItem(KEY) || "null");
    sessionStorage.removeItem(KEY);
  } catch {}
  const back = /** @type {PerformanceNavigationTiming | undefined} */ (performance.getEntriesByType("navigation")[0])?.type === "back_forward";
  // What is carried over is for the same user, within a flash cookie's life.
  const age = left && typeof left.at === "number" ? Date.now() - left.at : -1;
  if (!back && age >= 0 && age < KEPT_MS && left.who === user && Array.isArray(left.toasts)) {
    for (const spec of left.toasts.slice(0, 5)) {
      if (spec && typeof spec === "object") place(build(spec), Number(spec.elapsed) || 0);
    }
  }
  // Modals open and close around the toaster: it follows the top one.
  if (typeof MutationObserver === "function") {
    watcher = new MutationObserver((records) => {
      noted(records);
      reseat();
    });
    watcher.observe(root(), { subtree: true, attributes: true, attributeFilter: ["open"] });
  }
  modals = /** @type {HTMLDialogElement[]} */ ([...every("dialog:modal")]);
  reseat();
  on(document, "ui:swap", reseat);
  on(document, "animationend", (e) => {
    const t = /** @type {Element} */ (e.target);
    if (e.animationName === LIFE && t instanceof Element && attr(t, "data-slot") === "toast" && holds(host, t)) dismiss(t);
  });
  on(document, "click", (e) => {
    const t = closest(e.target, "[data-slot=toast]");
    if (!t || !holds(host, t)) return;
    const close = closest(e.target, "[data-slot=toast-close]");
    // The close link would go to its toast's address; here the runtime takes the toast away.
    if (close) e.preventDefault();
    if (close || closest(e.target, "a[data-slot=toast-action]")) dismiss(t);
  });
  // A post action is over when the server has answered; the answer may be a new toast.
  on(document, "ui:sent", (e) => {
    sameUser();
    const t = e.detail?.form instanceof Element ? Element.prototype.closest.call(e.detail.form, "[data-slot=toast]") : null;
    if (t && holds(host, t)) dismiss(t);
  });
  on(document, "visibilitychange", () => {
    host?.toggleAttribute("data-paused", hidden());
    if (!hidden()) sameUser();
  });
  // A dialog that closes takes the toaster with it, so it goes back to the page.
  on(document, "close", (e) => {
    if (host && e.target instanceof HTMLDialogElement) reseat();
  }, true);
  on(window, "pagehide", handOff);
  on(document, "keydown", (e) => {
    if (e.altKey && !e.ctrlKey && !e.metaKey && e.code === "KeyT") {
      const first = /** @type {HTMLElement | null} */ (host?.querySelector("[data-slot=toast] a,[data-slot=toast] button"));
      if (!first) return;
      e.preventDefault();
      if (!holds(host, active())) returnTo = active();
      first.focus();
    } else if (e.key === "Escape") {
      const t = closest(e.target, "[data-slot=toast]");
      // Inside a dialog the same key would close the dialog too: it has done its work here.
      if (t && holds(host, t)) {
        e.preventDefault();
        dismiss(t);
      }
    }
  });
  swipe();
}

/** Swiping a toast sideways with a finger or a pen takes it away; a short drag springs back. */
function swipe() {
  /** @type {{t: HTMLElement, x: number, dx: number, at: number, id: number} | null} */
  let drag = null;
  on(document, "pointerdown", (e) => {
    if (e.pointerType === "mouse" || !e.isPrimary) return;
    const t = /** @type {HTMLElement | null} */ (closest(e.target, "[data-slot=toast]"));
    if (!t || !holds(host, t)) return;
    drag = { t, x: e.clientX, dx: 0, at: e.timeStamp, id: e.pointerId };
    t.setAttribute("data-dragging", "");
  });
  on(document, "pointermove", (e) => {
    if (!drag || e.pointerId !== drag.id) return;
    drag.dx = e.clientX - drag.x;
    drag.t.style.translate = drag.dx + "px";
    drag.t.style.opacity = String(Math.max(0.2, 1 - Math.abs(drag.dx) / 240));
  });
  const end = (/** @type {PointerEvent} */ e) => {
    if (!drag || e.pointerId !== drag.id) return;
    const { t, dx, at } = drag;
    drag = null;
    t.removeAttribute("data-dragging");
    const fast = Math.abs(dx) / Math.max(1, e.timeStamp - at) > 0.5;
    t.setAttribute("data-swipe", "");
    if (e.type === "pointerup" && (Math.abs(dx) > 80 || (Math.abs(dx) > 30 && fast))) {
      t.style.translate = (dx > 0 ? "" : "-") + "110%";
      t.style.opacity = "0";
      setTimeout(() => dismiss(t), 200);
    } else {
      t.style.translate = "";
      t.style.opacity = "";
      setTimeout(() => t.removeAttribute("data-swipe"), 200);
    }
  };
  on(document, "pointerup", end);
  on(document, "pointercancel", end);
}
