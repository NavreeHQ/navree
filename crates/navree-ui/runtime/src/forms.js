// Forms: the pending state within the frame of the submit, then, on
// `data-ui-form`, a fetch whose answer is ops or a page.
import { applyEnvelope } from "./ops.js";
import { forget, go, pageSwaps, showPage, startRegions, whileWriting } from "./nav.js";
import { active, attr, cookies, has, on, set, unset, toggle } from "./dom.js";
import { toast } from "./ui.js";

const OPS = "application/vnd.navree.ops+json";

/**
 * The cookie called `name`, decoded. Another site on the same domain can set one
 * that doesn't decode, or that no header can carry: those are passed over.
 * @param {string} name
 */
function cookie(name) {
  for (const part of cookies().split("; ")) {
    if (!part.startsWith(name + "=")) continue;
    try {
      const value = decodeURIComponent(part.slice(name.length + 1));
      if (/^[\x21-\x7e]+$/.test(value)) return value;
    } catch {}
  }
  return "";
}

/** @param {HTMLFormElement} form @param {Element | null} submitter @param {boolean} on */
function pending(form, submitter, on) {
  toggle(form, "data-pending", on);
  if (submitter) toggle(submitter, "data-pending", on);
  if (on) set(form, "aria-busy", "true");
  else unset(form, "aria-busy");
}

/** Where and how a submit posts: the method and the whole address. @param {HTMLFormElement} form @param {HTMLElement | null} submitter */
const targetOf = (form, submitter) =>
  ((submitter && attr(submitter, "formmethod")) || attr(form, "method") || "get").toLowerCase() + " " + new URL((submitter && attr(submitter, "formaction")) || attr(form, "action") || location.href, location.href).href;

/**
 * What a submit would send: where, how, and every field. Two submits that say the same
 * are one: a double click, Enter twice.
 * @param {HTMLFormElement} form @param {HTMLElement | null} submitter
 */
function said(form, submitter) {
  const parts = [targetOf(form, submitter)];
  try {
    for (const [name, value] of new FormData(form, submitter)) {
      parts.push(name, typeof value === "string" ? value : `${value.name}:${value.size}:${value.lastModified}`);
    }
  } catch {}
  return JSON.stringify(parts);
}

/**
 * Forms on their way, with what they sent. The fetch of a write can't be taken back, so
 * a second submit that says the same is ignored. One that says something new, typed
 * while the first was on its way, waits: the latest only. It goes only where it was
 * typed, so it remembers the page, the address and the user it was typed for.
 * @type {WeakMap<HTMLFormElement, string>}
 */
const sending = new WeakMap();
/** @type {WeakMap<HTMLFormElement, {said: string, submitter: HTMLElement | null, page: number, href: string, target: string, token: string}>} */
const waiting = new WeakMap();

/** @param {HTMLFormElement} form @param {HTMLElement | null} submitter */
async function send(form, submitter) {
  sending.set(form, said(form, submitter));
  const before = active();
  let answered = false;
  /** Whether the answer left this page as it was: ops that don't lead away. */
  let kept = false;
  try {
    const action = (submitter && attr(submitter, "formaction")) || attr(form, "action") || location.href;
    const method = ((submitter && attr(submitter, "formmethod")) || attr(form, "method") || "get").toUpperCase();
    const data = new FormData(form, submitter);
    /** @type {Record<string, string>} */
    const headers = { "x-navree-ui": "1" };
    const token = cookie("XSRF-TOKEN");
    if (token) headers["x-xsrf-token"] = token;
    /** @type {RequestInit} */
    const init = { method, headers };
    const url = new URL(action, location.href);
    if (method === "GET") url.search = new URLSearchParams(/** @type {any} */ (data)).toString();
    else init.body = attr(form, "enctype") === "multipart/form-data" ? data : new URLSearchParams(/** @type {any} */ (data));
    const response = await (method === "GET" ? fetch(url, init) : whileWriting(() => fetch(url, init)));
    answered = true;
    const type = response.headers.get("content-type") || "";
    pending(form, submitter, false);
    document.dispatchEvent(new CustomEvent("ui:sent", { detail: { form, ok: response.ok } }));
    // Errors take focus only for a save the user asked for, and only if the user is
    // still where the save left them: never from a field they moved to, or typed on in.
    if (type.startsWith(OPS)) kept = !applyEnvelope(await response.json(), { focus: !!submitter && active() === before });
    else if (type.startsWith("text/html")) await showPage(response, { url: response.url, keep: response.url === location.href });
    else if (!response.ok) toast("danger", "That didn't work. Try again.");
    startRegions();
  } catch {
    pending(form, submitter, false);
    document.dispatchEvent(new CustomEvent("ui:sent", { detail: { form, ok: false } }));
    toast("danger", answered ? "That didn't work. Try again." : "No connection. Try again.");
  } finally {
    sending.delete(form);
    const next = waiting.get(form);
    waiting.delete(form);
    const by = next?.submitter?.isConnected ? next.submitter : null;
    // What was typed meanwhile goes now, from the form as it is, only where it was typed:
    // after ops that left the page alone, on the same page and address, to the same
    // place, for the same user. A redirect, a page, a swap, a new action or a sign-out
    // drops it.
    if (next && kept && form.isConnected && next.page === pageSwaps() && next.href === location.href && next.target === targetOf(form, by) && next.token === cookie("XSRF-TOKEN")) {
      try {
        form.requestSubmit(/** @type {HTMLButtonElement | null} */ (by));
      } catch {
        form.requestSubmit();
      }
    }
  }
}

/** How long typing in `form` waits for a pause, in ms: its `data-ui-debounce`, or 100. @param {HTMLFormElement} form */
function delay(form) {
  const ms = Number(attr(form, "data-ui-debounce"));
  return Number.isFinite(ms) && ms >= 0 && attr(form, "data-ui-debounce") !== null ? Math.min(ms, 5000) : 100;
}

/** A GET form with a region: filters that morph the region in place. @param {HTMLFormElement} form */
function filter(form) {
  const url = new URL(attr(form, "action") || location.href, location.href);
  const params = new URLSearchParams();
  // An empty control says nothing: it stays out of the URL.
  for (const [name, value] of new FormData(form)) if (value !== "" || name === "sort" || name === "fb") params.append(name, /** @type {string} */ (value));
  url.search = params.toString();
  go(url.href, { region: attr(form, "data-ui-target") || undefined }).then(startRegions);
}

/** Wires up forms. */
export function startForms() {
  on(document, "submit", (e) => {
    const form = /** @type {HTMLFormElement} */ (e.target);
    if (e.defaultPrevented) return;
    const submitter = /** @type {HTMLElement | null} */ (e.submitter);
    if (has(form, "data-ui-target")) {
      e.preventDefault();
      filter(form);
      return;
    }
    const out = sending.get(form);
    if (out !== undefined) {
      e.preventDefault();
      const now = said(form, submitter);
      if (now !== out) waiting.set(form, { said: now, submitter, page: pageSwaps(), href: location.href, target: targetOf(form, submitter), token: cookie("XSRF-TOKEN") });
      else waiting.delete(form);
      return;
    }
    const method = ((submitter && attr(submitter, "formmethod")) || attr(form, "method") || "get").toLowerCase();
    if (method !== "dialog") pending(form, submitter, true);
    if (has(form, "data-ui-form")) {
      e.preventDefault();
      send(form, submitter);
    } else if (method === "post") forget();
  });
  /** @type {WeakMap<HTMLFormElement, number>} */
  const timers = new WeakMap();
  on(document, "input", (e) => {
    const input = /** @type {HTMLInputElement} */ (e.target);
    const form = input.form;
    if (!form || !has(form, "data-ui-target") || /** @type {InputEvent} */ (e).isComposing) return;
    clearTimeout(timers.get(form));
    // A select says so again in `change`, below.
    if (input.nodeName === "SELECT") return;
    // A tick or a choice is finished when it's made: it applies at once. Text waits
    // for a pause in the typing, 100 ms unless the form says otherwise.
    if (input.type === "checkbox" || input.type === "radio") filter(form);
    else timers.set(form, setTimeout(() => filter(form), delay(form)));
  });
  on(document, "change", (e) => {
    const input = /** @type {HTMLInputElement} */ (e.target);
    const owner = input.form;
    if (owner && has(owner, "data-ui-target") && input.nodeName === "SELECT") filter(owner);
  });
}
