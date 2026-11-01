// Forms: the pending state within the frame of the submit, then, on
// `data-ui-form`, a fetch whose answer is ops or a page.
import { applyEnvelope } from "./ops.js";
import { forgetPrefetched, go, showPage, startRegions } from "./nav.js";
import { attr, has, set, unset, toggle, toast } from "./ui.js";

const OPS = "application/vnd.navree.ops+json";

/** @param {string} name */
function cookie(name) {
  for (const part of document.cookie.split("; ")) {
    if (part.startsWith(name + "=")) return decodeURIComponent(part.slice(name.length + 1));
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

/** The running submit of each form. @type {WeakMap<HTMLFormElement, AbortController>} */
const running = new WeakMap();

/** @param {HTMLFormElement} form @param {HTMLElement | null} submitter */
async function send(form, submitter) {
  running.get(form)?.abort();
  const abort = new AbortController();
  running.set(form, abort);
  const action = (submitter && attr(submitter, "formaction")) || attr(form, "action") || location.href;
  const method = ((submitter && attr(submitter, "formmethod")) || attr(form, "method") || "get").toUpperCase();
  const data = new FormData(form, submitter);
  /** @type {Record<string, string>} */
  const headers = { "x-navree-ui": "1" };
  const token = cookie("XSRF-TOKEN");
  if (token) headers["x-xsrf-token"] = token;
  /** @type {RequestInit} */
  const init = { method, headers, signal: abort.signal };
  const url = new URL(action, location.href);
  if (method === "GET") url.search = new URLSearchParams(/** @type {any} */ (data)).toString();
  else init.body = attr(form, "enctype") === "multipart/form-data" ? data : new URLSearchParams(/** @type {any} */ (data));
  try {
    const response = await fetch(url, init);
    const type = response.headers.get("content-type") || "";
    pending(form, submitter, false);
    if (method !== "GET") forgetPrefetched();
    document.dispatchEvent(new CustomEvent("ui:sent", { detail: { form, ok: response.ok } }));
    if (type.startsWith(OPS)) applyEnvelope(await response.json());
    else if (type.startsWith("text/html")) await showPage(response, { url: response.url });
    else if (!response.ok) toast("danger", "That didn't work. Try again.");
    startRegions();
  } catch (error) {
    if (/** @type {Error} */ (error).name === "AbortError") return;
    pending(form, submitter, false);
    toast("danger", "No connection. Try again.");
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
  document.addEventListener("submit", (e) => {
    const form = /** @type {HTMLFormElement} */ (e.target);
    if (e.defaultPrevented) return;
    const submitter = /** @type {HTMLElement | null} */ (e.submitter);
    if (has(form, "data-ui-target")) {
      e.preventDefault();
      filter(form);
      return;
    }
    if ((attr(form, "method") || "").toLowerCase() !== "dialog") pending(form, submitter, true);
    if (has(form, "data-ui-form")) {
      e.preventDefault();
      send(form, submitter);
    }
  });
  /** @type {WeakMap<HTMLFormElement, number>} */
  const timers = new WeakMap();
  document.addEventListener("input", (e) => {
    const form = /** @type {HTMLInputElement} */ (e.target).form;
    if (!form || !has(form, "data-ui-target") || /** @type {InputEvent} */ (e).isComposing) return;
    clearTimeout(timers.get(form));
    timers.set(form, setTimeout(() => filter(form), 200));
  });
  document.addEventListener("change", (e) => {
    const input = /** @type {HTMLInputElement} */ (e.target);
    const owner = input.form;
    if (owner && has(owner, "data-ui-target") && input.nodeName === "SELECT") filter(owner);
  });
}
