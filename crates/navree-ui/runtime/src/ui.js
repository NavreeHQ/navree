// What ops and commands share: opening things, toasts, the announcer.
//
// A toast lives in a CSS animation (`ui-toast-life`, as long as its `data-duration`)
// and ends on `animationend`; pausing under the pointer or focus is
// `animation-play-state`, so there is no timer to lose and nothing that can stick.
import { body, hidden, make, on, one, persistent } from "./dom.js";

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

/** @type {HTMLElement | undefined} */
let host;
/** Where focus was before a hotkey took it into the toasts. @type {Element | null} */
let returnTo = null;

/** The toasts' one standing place: the page's own, or one made now. It stays put through every navigation. */
function toaster() {
  if (host?.isConnected) return host;
  // Only the page's own, a direct child of the body: stored markup can't stand in for it.
  host = /** @type {HTMLElement | undefined} */ ([...body().children].find((el) => el.getAttribute("data-slot") === "toaster"));
  if (!host) {
    host = make("section");
    host.setAttribute("data-slot", "toaster");
    host.setAttribute("aria-label", "Notifications");
    host.setAttribute("data-position", "bottom-end");
    host.setAttribute("data-close", "Dismiss");
  }
  if (!host.parentNode) body().append(host);
  persistent.add(host);
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
  if (spec.closable !== false) {
    const close = make("button");
    close.setAttribute("data-slot", "toast-close");
    close.setAttribute("type", "button");
    close.setAttribute("aria-label", toaster().getAttribute("data-close") || "Dismiss");
    el.append(close);
  }
  return el;
}

/** Puts `el` in the standing toaster, in place of a toast with its name. @param {HTMLElement} el @param {number} [elapsed] */
function place(el, elapsed) {
  const box = toaster();
  const id = el.getAttribute("data-toast-id");
  const old = id && [...box.children].find((t) => t.getAttribute("data-toast-id") === id);
  if (old) old.replaceWith(el);
  else box.append(el);
  show(box);
  if (elapsed) {
    const life = el.getAnimations().find((a) => /** @type {CSSAnimation} */ (a).animationName === LIFE);
    if (life) life.currentTime = elapsed;
  }
}

/**
 * Opens the toaster. While a modal dialog is open, everything outside it is inert, so the
 * toaster stands inside the dialog to stay above it and usable, and comes back out when it closes.
 * @param {HTMLElement} box
 */
function show(box) {
  const modal = one("dialog:modal");
  try {
    if (modal && !modal.contains(box)) {
      modal.append(box);
    } else if (!modal && box.parentNode !== body()) {
      body().append(box);
    }
    if (!box.matches(":popover-open")) box.showPopover();
  } catch {}
}

/** Takes a toast away, and focus with it to where it came from. @param {Element} t */
function dismiss(t) {
  const held = t.contains(document.activeElement);
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
  const from = [...body(doc).children].find((el) => el.getAttribute("data-slot") === "toaster");
  if (!from) return;
  const box = toaster();
  const position = from.getAttribute("data-position");
  if (position) box.setAttribute("data-position", position);
  const close = from.getAttribute("data-close");
  if (close) box.setAttribute("data-close", close);
  const kids = [...from.children].filter((t) => t.getAttribute("data-slot") === "toast");
  from.remove();
  for (const kid of kids) place(/** @type {HTMLElement} */ (kid));
}

/** What a showing toast says, to show again on the next page. @param {Element} t */
function specOf(t) {
  const text = (/** @type {string} */ s) => t.querySelector(s)?.textContent || undefined;
  const link = t.querySelector("a[data-slot=toast-action]");
  const form = t.querySelector("form");
  const action = link
    ? { label: link.textContent || "", href: link.getAttribute("href") || "" }
    : form ? { label: form.textContent || "", href: form.getAttribute("action") || "", post: true } : undefined;
  return {
    tone: t.getAttribute("data-tone"),
    text: text("[data-slot=toast-text]"),
    title: text("[data-slot=toast-title]"),
    id: t.getAttribute("data-toast-id") || undefined,
    duration: t.getAttribute("data-duration"),
    closable: !!t.querySelector("[data-slot=toast-close]"),
    action,
  };
}

/** Writes the toasts still showing, with the time they have left, for the page that comes next. */
function handOff() {
  if (!host?.isConnected) return;
  const list = [];
  for (const t of host.querySelectorAll("[data-slot=toast]")) {
    const life = t.getAnimations().find((a) => /** @type {CSSAnimation} */ (a).animationName === LIFE);
    list.push({ ...specOf(t), elapsed: life ? Number(life.currentTime) || 0 : 0 });
  }
  try {
    if (list.length) sessionStorage.setItem(KEY, JSON.stringify(list));
  } catch {}
}

/** Starts the toasts: marks the page's own toaster, shows what the last page left, and listens. */
export function startToasts() {
  const box = toaster();
  /** @type {any[]} */
  let left = [];
  try {
    left = JSON.parse(sessionStorage.getItem(KEY) || "[]");
    sessionStorage.removeItem(KEY);
  } catch {}
  const back = /** @type {PerformanceNavigationTiming | undefined} */ (performance.getEntriesByType("navigation")[0])?.type === "back_forward";
  if (Array.isArray(left) && !back) {
    for (const spec of left.slice(0, 5)) {
      if (spec && typeof spec === "object") place(build(spec), Number(spec.elapsed) || 0);
    }
  }
  if (box.querySelector("[data-slot=toast]")) show(box);
  on(document, "animationend", (e) => {
    const t = /** @type {Element} */ (e.target);
    if (e.animationName === LIFE && t.getAttribute?.("data-slot") === "toast" && host?.contains(t)) dismiss(t);
  });
  on(document, "click", (e) => {
    const t = /** @type {Element} */ (e.target).closest?.("[data-slot=toast]");
    if (!t || !host?.contains(t)) return;
    if (/** @type {Element} */ (e.target).closest("[data-slot=toast-close],a[data-slot=toast-action]")) dismiss(t);
  });
  // A post action is over when the server has answered; the answer may be a new toast.
  on(document, "ui:sent", (e) => {
    const t = e.detail?.form instanceof Element ? Element.prototype.closest.call(e.detail.form, "[data-slot=toast]") : null;
    if (t && host?.contains(t)) dismiss(t);
  });
  on(document, "visibilitychange", () => host?.toggleAttribute("data-paused", hidden()));
  // A dialog that closes takes the toaster with it, so it goes back to the page.
  on(document, "close", (e) => {
    if (host && e.target instanceof HTMLDialogElement && e.target.contains(host)) {
      body().append(host);
      if (host.querySelector("[data-slot=toast]")) show(host);
    }
  }, true);
  on(window, "pagehide", handOff);
  on(document, "keydown", (e) => {
    if (e.altKey && !e.ctrlKey && !e.metaKey && e.code === "KeyT") {
      const first = /** @type {HTMLElement | null} */ (host?.querySelector("[data-slot=toast] button,[data-slot=toast] a"));
      if (!first) return;
      e.preventDefault();
      if (!host?.contains(document.activeElement)) returnTo = document.activeElement;
      first.focus();
    } else if (e.key === "Escape") {
      const t = /** @type {Element} */ (e.target).closest?.("[data-slot=toast]");
      if (t && host?.contains(t)) dismiss(t);
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
    const t = /** @type {HTMLElement | null} */ (/** @type {Element} */ (e.target).closest?.("[data-slot=toast]"));
    if (!t || !host?.contains(t)) return;
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
