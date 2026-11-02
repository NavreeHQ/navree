// Forms: the pending state within the frame of the submit, then, on
// `data-ui-form`, a fetch whose answer is ops or a page.
import { applyEnvelope } from "./ops.js";
import { forget, go, showPage, startRegions, whileWriting } from "./nav.js";
import { attr, cookies, has, on, set, unset, toggle } from "./dom.js";
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

/** Forms on their way: the fetch of a write can't be taken back, so a second submit is ignored. @type {WeakSet<HTMLFormElement>} */
const sending = new WeakSet();

/** @param {HTMLFormElement} form @param {HTMLElement | null} submitter */
async function send(form, submitter) {
  sending.add(form);
  let answered = false;
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
    if (type.startsWith(OPS)) applyEnvelope(await response.json());
    else if (type.startsWith("text/html")) await showPage(response, { url: response.url, keep: response.url === location.href });
    else if (!response.ok) toast("danger", "That didn't work. Try again.");
    startRegions();
  } catch {
    pending(form, submitter, false);
    toast("danger", answered ? "That didn't work. Try again." : "No connection. Try again.");
  } finally {
    sending.delete(form);
  }
}

/** A GET form with a region: filters that morph the region in place. @param {HTMLFormElement} form */
function filter(form) {
  const url = new URL(attr(form, "action") || location.href, location.href);
  const params = new URLSearchParams();
  // An empty control says nothing: it stays out of the URL.
  for (const [name, value] of new FormData(form)) if (value !== "" || name === "sort") params.append(name, /** @type {string} */ (value));
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
    if (sending.has(form)) {
      e.preventDefault();
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
    const form = /** @type {HTMLInputElement} */ (e.target).form;
    if (!form || !has(form, "data-ui-target") || /** @type {InputEvent} */ (e).isComposing) return;
    clearTimeout(timers.get(form));
    timers.set(form, setTimeout(() => filter(form), 200));
  });
  on(document, "change", (e) => {
    const input = /** @type {HTMLInputElement} */ (e.target);
    const owner = input.form;
    if (owner && has(owner, "data-ui-target") && input.nodeName === "SELECT") filter(owner);
  });
}
