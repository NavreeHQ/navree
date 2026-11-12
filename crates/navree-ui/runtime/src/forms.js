// Forms: the pending state within the frame of the submit, then, on
// `data-ui-form`, a fetch whose answer is ops or a page.
import { applyEnvelope } from "./ops.js";
import { forget, go, pageSwaps, showPage, startRegions, whileWriting } from "./nav.js";
import { active, all, attr, body, byId, closest, cookies, emit, has, make, on, sameOrigin, set, unset, toggle } from "./dom.js";
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
 * are one: a double click, Enter twice. A file is its name, size, type and time, so one
 * picked again is new; an empty one is nothing, though a browser stamps it with the time.
 * @param {HTMLFormElement} form @param {HTMLElement | null} submitter
 */
function said(form, submitter) {
  const parts = [targetOf(form, submitter)];
  try {
    for (const [name, value] of fieldsOf(form, submitter)) {
      parts.push(name, typeof value === "string" ? value : value.size || value.name ? `${value.name}:${value.size}:${value.type}:${value.lastModified}` : "");
    }
  } catch {}
  return JSON.stringify(parts);
}

const gElements = /** @type {(this: HTMLFormElement) => HTMLFormControlsCollection} */ (Object.getOwnPropertyDescriptor(HTMLFormElement.prototype, "elements")?.get);

/**
 * Which record a form edits: its row's key and its hidden fields, the token aside. A list
 * without keys reuses a form for another row when it is drawn again.
 * @param {HTMLFormElement} form
 */
function recordOf(form) {
  const row = Element.prototype.closest.call(form, "[data-key]");
  const parts = [row && attr(row, "data-key")];
  for (const field of gElements.call(form)) if (field instanceof HTMLInputElement && field.type === "hidden" && field.name !== "_csrf") parts.push(field.name, field.value);
  return JSON.stringify(parts);
}

/**
 * Forms on their way, with what they sent. The fetch of a write can't be taken back, so
 * a second submit that says the same is ignored. One that says something new, typed
 * while the first was on its way, waits: the latest only. It goes only where it was
 * typed, so it remembers the page, the address and the user it was typed for. A form is
 * one send; the page's post form is shared by every post item, so there it is the item
 * that is one.
 * @type {WeakMap<Element, string>}
 */
const sending = new WeakMap();
/** @type {WeakMap<Element, {said: string, submitter: HTMLElement | null, page: number, href: string, target: string, token: string, record: string}>} */
const waiting = new WeakMap();

/** What a send is counted as. @param {HTMLFormElement} form @param {Element | null} submitter */
const sendOf = (form, submitter) => (submitter && attr(form, "id") === "ui-post" ? submitter : form);

/**
 * The fields of a send. The page's post form carries only the token it holds: another element
 * of the page that names it with `form` joins no post.
 * @param {HTMLFormElement} form @param {HTMLElement | null} submitter
 */
function fieldsOf(form, submitter) {
  if (attr(form, "id") !== "ui-post") return new FormData(form, submitter);
  const data = new FormData();
  for (const input of all(form, "input[type=hidden][name=_csrf]")) data.append("_csrf", /** @type {HTMLInputElement} */ (input).value);
  return data;
}

/** Where a send goes: the submitter's `formaction`, the form's `action`, or this page. @param {HTMLFormElement} form @param {HTMLElement | null} submitter */
const actionOf = (form, submitter) => (submitter && attr(submitter, "formaction")) || attr(form, "action") || location.href;

/** @param {HTMLFormElement} form @param {HTMLElement | null} submitter */
async function send(form, submitter) {
  const one = sendOf(form, submitter);
  sending.set(one, said(form, submitter));
  const before = active();
  let answered = false, status = 0;
  /** Whether the answer left this page as it was: ops that don't lead away. */
  let kept = false;
  /** Whether the user was told the save failed. */
  let told = false;
  /** Whether the send was said to be over, and whether a failure is this runtime's to tell. */
  let ended = false, mine = true;
  /**
   * Says the send is over. A module that cancels the event tells the user what went
   * wrong itself: a sortable asks the server where the item is first.
   * @param {boolean} ok
   */
  const over = (ok) => {
    ended = true;
    return emit(new CustomEvent("ui:sent", { cancelable: true, detail: { form, ok, status } }));
  };
  try {
    const action = actionOf(form, submitter);
    // Only this site's actions are fetched, as by every other fetch of the runtime: a form
    // aimed at another origin is refused before anything is sent. A redirect is another
    // matter (below): fetch follows it before the runtime sees it.
    if (!sameOrigin(action)) {
      pending(form, submitter, false);
      if (over(false)) toast("danger", "That didn't work. Try again.");
      told = true;
      return;
    }
    const method = ((submitter && attr(submitter, "formmethod")) || attr(form, "method") || "get").toUpperCase();
    const data = fieldsOf(form, submitter);
    /** @type {Record<string, string>} */
    const headers = { "x-navree-ui": "1", "x-navree-from": location.pathname + location.search };
    const token = cookie("XSRF-TOKEN");
    if (token) headers["x-xsrf-token"] = token;
    /** @type {RequestInit} */
    const init = { method, headers };
    // A form that says how long it waits gives up then, and counts as failed: a move put back, not one left pending.
    const wait = Number(attr(form, "data-ui-deadline"));
    if (wait > 0 && typeof AbortSignal.timeout === "function") init.signal = AbortSignal.timeout(wait);
    const url = new URL(action, location.href);
    if (method === "GET") url.search = new URLSearchParams(/** @type {any} */ (data)).toString();
    else init.body = attr(form, "enctype") === "multipart/form-data" ? data : new URLSearchParams(/** @type {any} */ (data));
    const response = await (method === "GET" ? fetch(url, init) : whileWriting(() => fetch(url, init)));
    answered = true;
    // An answer redirected off this site is refused before its body is read: no ops, no page
    // from it. The request itself has gone there by then, headers (the session token, the
    // page's path) and, after a 307 or 308, the body, if that origin answered the browser's
    // preflight. Navree's page policy (`connect-src 'self'`) stops that hop in the browser;
    // a page served without it, with a same-origin action that redirects off the site, does not.
    if (response.redirected && !sameOrigin(response.url)) throw new Error("redirected off this site");
    status = response.status;
    const type = response.headers.get("content-type") || "";
    pending(form, submitter, false);
    mine = over(response.ok);
    // Errors take focus only for a save the user asked for, and only if the user is
    // still where the save left them: never from a field they moved to, or typed on in.
    if (type.startsWith(OPS)) kept = !applyEnvelope(await response.json(), { focus: !!submitter && active() === before });
    else if (type.startsWith("text/html")) await showPage(response, { url: response.url, keep: response.url === location.href });
    else if (!response.ok && mine) toast("danger", "That didn't work. Try again.");
    startRegions();
  } catch {
    pending(form, submitter, false);
    if (!ended) mine = over(false);
    if (mine) toast("danger", answered ? "That didn't work. Try again." : "No connection. Try again.");
    told = true;
  } finally {
    sending.delete(one);
    const next = waiting.get(one);
    waiting.delete(one);
    const by = next?.submitter?.isConnected ? next.submitter : null;
    // What was typed meanwhile goes now, from the form as it is, only where it was typed:
    // after ops that left the page alone, on the same page and address, to the same
    // place and record, for the same user. A redirect, a page, a swap, a new action, a
    // form drawn again for another record or a sign-out drops it, and the user is told.
    if (next && kept && form.isConnected && next.page === pageSwaps() && next.href === location.href && next.target === targetOf(form, by) && next.token === cookie("XSRF-TOKEN") && next.record === recordOf(form)) {
      try {
        form.requestSubmit(/** @type {HTMLButtonElement | null} */ (by));
      } catch {
        form.requestSubmit();
      }
    } else if (next && !told) toast("warning", "Your latest change wasn't saved. Check it and save again.");
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
  // A menu's post item belongs to the page's one post form, `ui-post`, and says where to post.
  // A page that doesn't have it (the item came in with a fragment) gets it now, ahead of the
  // click's own submit: the runtime posts it with the session's token in the header.
  on(document, "click", (e) => {
    if (!closest(e.target, "button[form=ui-post]") || byId("ui-post")) return;
    const form = make("form");
    set(form, "id", "ui-post");
    set(form, "method", "post");
    // Not validated: a field elsewhere that names it, and isn't valid, must not hold back a post.
    set(form, "novalidate", "");
    set(form, "hidden", "");
    set(form, "data-ui-form", "");
    body().append(form);
  }, true);
  on(document, "submit", (e) => {
    const form = /** @type {HTMLFormElement} */ (e.target);
    if (e.defaultPrevented) return;
    const submitter = /** @type {HTMLElement | null} */ (e.submitter);
    if (has(form, "data-ui-target")) {
      e.preventDefault();
      filter(form);
      return;
    }
    const one = sendOf(form, submitter);
    const out = sending.get(one);
    if (out !== undefined) {
      e.preventDefault();
      const now = said(form, submitter);
      if (now !== out) waiting.set(one, { said: now, submitter, page: pageSwaps(), href: location.href, target: targetOf(form, submitter), token: cookie("XSRF-TOKEN"), record: recordOf(form) });
      else waiting.delete(one);
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
