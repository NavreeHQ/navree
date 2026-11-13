// navree.com's preview frame. A preview shows a component, not the app behind
// it: there is no server, so its links, forms and requests go nowhere. This runs
// before the toolkit's modules and in the capture phase, so they see the event
// as already handled, and it says what the app would have done instead.

/** The previews' own folder: their pages and the toolkit's files. Nothing else here is served. */
const ROOT = new URL("..", location.href).pathname;

// A form is read through the prototypes: a field named `action` or `matches` can't stand in.
const E = Element.prototype;
const attr = (el, name) => E.getAttribute.call(el, name);
const has = (el, name) => E.hasAttribute.call(el, name);
const is = (el, selector) => E.matches.call(el, selector);
const find = (el, selector) => E.querySelector.call(el, selector);

/** An address as a reader would write it: the path and query on this site, else the whole of it. */
const written = (url) => (url.origin === location.origin ? url.pathname + url.search : url.href);

/** Whether the reader has just clicked, tapped or typed: what they did, not what a module did by itself. */
const theirs = () => navigator.userActivation?.isActive !== false;

// ---- The note: what the app would have done ----

/** @type {HTMLElement | null} */
let note = null;
let hide = 0;

/**
 * Says, for a few seconds, what an app would have done here: `before`, the address
 * as code, then `after`. A status, so assistive technology reads it too.
 */
function tell(before, url, after) {
  if (!note) {
    note = document.createElement("div");
    note.setAttribute("data-preview-note", "");
    note.setAttribute("role", "status");
    note.setAttribute("popover", "manual");
    document.body.append(note);
  }
  const code = document.createElement("code");
  code.textContent = written(url);
  try {
    // Shown again, so it is over anything opened since.
    if (note.matches(":popover-open")) note.hidePopover();
    note.showPopover();
  } catch {}
  note.replaceChildren(before, code, after);
  clearTimeout(hide);
  hide = setTimeout(() => {
    try {
      note?.hidePopover();
    } catch {}
  }, 5000);
}

const NO_SERVER = "This preview has no server. In your app, ";

// ---- Links ----

addEventListener(
  "click",
  (event) => {
    const target = event.target instanceof Element ? event.target : null;
    // A tab is a link the toolkit switches in place, so its click is left alone.
    const link = target?.closest("a[href]:not([role=tab])");
    if (link) {
      event.preventDefault();
      const href = attr(link, "href") || "";
      // A list's "Load more" and a toast's close are the toolkit's to handle in place.
      const inPlace = href.startsWith("#") || is(link, "[data-slot=lazy-more], [data-slot=toast-close]");
      if (!inPlace) tell(`${NO_SERVER}this link opens `, new URL(href, location.href), ".");
      return;
    }
    // A submit button: the browser would post the form, which a preview can't. Its
    // click is the one place every way of pressing it (a click, Enter, Space) passes.
    const button = target?.closest("button, input[type=submit], input[type=image]");
    if (!button || (button.type !== "submit" && button.type !== "image")) return;
    if (!button.form) {
      // A menu's post item names the page's one post form, which a preview doesn't draw.
      const action = attr(button, "formaction");
      if (!action) return;
      event.preventDefault();
      tell(`${NO_SERVER}this sends ${(attr(button, "formmethod") || "post").toUpperCase()} `, new URL(action, location.href), ".");
      return;
    }
    event.preventDefault();
    send(button.form, button, true);
  },
  true,
);

// ---- Forms ----

/** Where `form` would go: its method, and its address with a GET's fields as the query. */
function destination(form, submitter) {
  const method = ((submitter && attr(submitter, "formmethod")) || attr(form, "method") || "get").toLowerCase();
  const url = new URL((submitter && attr(submitter, "formaction")) || attr(form, "action") || location.href, location.href);
  if (method === "get") {
    let data;
    try {
      data = new FormData(form, submitter || undefined);
    } catch {
      data = new FormData(form);
    }
    // An empty field says nothing: it stays out of the address, as the toolkit leaves it out.
    const query = new URLSearchParams();
    for (const [name, value] of data) if (typeof value === "string" && value !== "") query.append(name, value);
    url.search = query.toString();
  }
  return { method, url };
}

/**
 * What a submit does in a preview: the browser's own checks first, then the theme
 * menu applies its choice in place, and any other form says what it would send.
 * A form the toolkit sends itself (`data-ui-form`) hears that it was taken, as the
 * server would say: a moved card stays, a toast's action takes the toast away.
 */
function send(form, submitter, validate) {
  const checked = validate && !has(form, "novalidate") && !(submitter && has(submitter, "formnovalidate"));
  if (checked && !HTMLFormElement.prototype.reportValidity.call(form)) return;
  const { method, url } = destination(form, submitter);
  if (method === "dialog") {
    E.closest.call(form, "dialog")?.close(submitter?.value ?? "");
    return;
  }
  if (is(form, "[data-slot=theme-menu-form]")) {
    look(form);
    return;
  }
  if (theirs()) {
    if (method !== "get") tell(`${NO_SERVER}this sends ${method.toUpperCase()} `, url, ".");
    else if (has(form, "data-ui-target")) tell(`${NO_SERVER}this asks for `, url, " and draws the answer in place.");
    else tell(`${NO_SERVER}this opens `, url, ".");
  }
  if (has(form, "data-ui-form")) {
    setTimeout(() => document.dispatchEvent(new CustomEvent("ui:sent", { cancelable: true, detail: { form, ok: true, status: 200 } })));
  }
}

// A module's own submit (a sortable's move, a list's next page) comes here too, and so
// does `form.submit()`, which no submit event would catch.
HTMLFormElement.prototype.requestSubmit = function (submitter) {
  send(this, submitter || null, true);
};
HTMLFormElement.prototype.submit = function () {
  send(this, null, false);
};
// What a form's own submit event would still do, should one fire, goes nowhere.
addEventListener("submit", (event) => event.preventDefault(), true);

// A form that redraws a part of the page as it changes (a filter, the theme
// builder) asks the server for that part. Without the toolkit's core nothing
// would happen at all, so the note says what the app would ask for.
let typing = 0;
for (const kind of ["change", "input"]) {
  addEventListener(kind, (event) => {
    const field = event.target;
    const form = field instanceof Element ? field.form || field.closest("form") : null;
    if (!form || document.documentElement.hasAttribute("data-ui-js")) return;
    if (is(form, "[data-slot=theme-menu-form]")) {
      if (kind === "change") look(form);
      return;
    }
    if (!has(form, "data-ui-target")) return;
    const text = field instanceof HTMLInputElement && !["radio", "checkbox", "color"].includes(field.type);
    // A choice says so when it is made, a box when the typing stops; each change once.
    if ((kind === "input") !== text) return;
    clearTimeout(typing);
    typing = setTimeout(() => tell(`${NO_SERVER}this asks for `, destination(form, null).url, " and draws the answer in place."), text ? 400 : 0);
  }, true);
}

/**
 * The theme menu's choice, applied in place, as the app draws its next page: the
 * theme and mode on `<html>`, and each card's swatches in the mode.
 */
function look(form) {
  const html = document.documentElement;
  const theme = find(form, "input[name=theme]:checked");
  const mode = find(form, "input[name=mode]:checked");
  const key = theme?.closest("[data-slot=theme-card]")?.querySelector("[data-theme]")?.getAttribute("data-theme") || theme?.value;
  if (key) html.setAttribute("data-theme", key);
  else if (mode && !html.hasAttribute("data-theme")) html.setAttribute("data-theme", "navree");
  if (!mode) return;
  html.setAttribute("data-mode", mode.value);
  document.querySelector("meta[name=color-scheme]")?.setAttribute("content", mode.value === "system" ? "light dark" : mode.value);
  for (const swatches of E.querySelectorAll.call(form, "[data-slot=theme-swatches]")) swatches.setAttribute("data-mode", mode.value);
}

// ---- Requests ----

// A list's next pages, drawn ahead of time by the app's own Rust: what its source
// would answer, taken in order. A request the preview has no answer for gets an
// empty one: a list ends, a save is taken, and nothing is asked of a server that
// isn't there.
const answers = [...document.querySelectorAll("template[data-preview-answer]")];
const fetched = window.fetch;
const page = (html, more) =>
  new Response(html, { status: 200, headers: { "content-type": "text/html; charset=utf-8", "x-ui-more": more ? "1" : "0" } });
window.fetch = function (input, init) {
  let url;
  try {
    url = new URL(input instanceof Request ? input.url : String(input), location.href);
  } catch {
    return fetched.call(window, input, init);
  }
  if (url.origin !== location.origin || url.pathname.startsWith(ROOT)) return fetched.call(window, input, init);
  const method = (init?.method || (input instanceof Request ? input.method : "GET")).toUpperCase();
  const dir = url.searchParams.get("dir");
  if (method === "GET" && dir) {
    const at = answers.findIndex((answer) => answer.dataset.src === url.pathname && answer.dataset.dir === dir);
    if (at >= 0) {
      const [answer] = answers.splice(at, 1);
      return Promise.resolve(page(answer.innerHTML, answer.dataset.more === "1"));
    }
    return Promise.resolve(page("", false));
  }
  if (method !== "GET" && theirs()) tell(`${NO_SERVER}this sends ${method} `, url, ".");
  return Promise.resolve(new Response(null, { status: 204 }));
};

// Nothing loads another page into the frame: a script that would (a part of the page
// that falls back to a whole page, say) is stopped, and the note says where it went.
if ("navigation" in window) {
  window.navigation.addEventListener("navigate", (event) => {
    if (event.destination.sameDocument || !event.cancelable) return;
    const url = new URL(event.destination.url);
    if (url.origin === location.origin && url.pathname.startsWith(ROOT)) return;
    event.preventDefault();
    tell(`${NO_SERVER}this opens `, url, ".");
  });
}

// ---- Pictures drawn in place ----

// A preview that shows a menu, list, card or dialog as it looks open draws the real
// element in place. Opened for real, it moves to the top layer and leaves the page:
// a stand-in of its size keeps its room meanwhile, so the control that opened it
// stays under the pointer, and a hover card doesn't close as its trigger moves away.
const PICTURES =
  "[data-slot=dialog],[data-slot=alert-dialog],[data-slot=sheet],[data-slot=drawer],[data-slot=select-popup],[data-slot=command-dialog],[data-slot=popover-content],[data-slot=hover-card-content],[data-slot=info-tip-content],[data-slot=tooltip-content],[data-slot=navigation-menu-content],[data-slot=dropdown-menu]";
if (document.body.hasAttribute("data-preview-show")) {
  /** @type {Map<Element, HTMLElement>} */
  const kept = new Map();
  /** The room `el` takes where it is drawn, margins too; nothing when it isn't drawn in place. */
  const roomOf = (el) => {
    const style = getComputedStyle(el);
    if (style.position !== "relative" || style.display === "none") return null;
    const box = el.getBoundingClientRect();
    const px = (value) => Number.parseFloat(value) || 0;
    return {
      width: box.width + px(style.marginLeft) + px(style.marginRight),
      height: box.height + px(style.marginTop) + px(style.marginBottom),
    };
  };
  const keep = (el, room) => {
    if (kept.has(el) || !room) return;
    const stand = document.createElement("div");
    stand.setAttribute("data-preview-kept", "");
    stand.setAttribute("aria-hidden", "true");
    stand.style.width = `${room.width}px`;
    stand.style.height = `${room.height}px`;
    el.before(stand);
    kept.set(el, stand);
  };
  const free = (el) => {
    kept.get(el)?.remove();
    kept.delete(el);
  };
  // A popover is measured as it is about to open, while it is still in place, and
  // gives its room back as it is about to close, so the page never moves between.
  addEventListener(
    "beforetoggle",
    (event) => {
      const el = event.target;
      if (!(el instanceof Element) || !el.matches(PICTURES)) return;
      if (event.newState === "open") keep(el, roomOf(el));
      else free(el);
    },
    true,
  );
  // A dialog opened with showModal() may say nothing before it opens, so its room is
  // read while it is in place, and its stand-in goes in before the next paint.
  const dialogs = [...document.querySelectorAll(PICTURES)].filter((el) => el instanceof HTMLDialogElement);
  /** @type {WeakMap<Element, {width: number, height: number}>} */
  const rooms = new WeakMap();
  const sizes = new ResizeObserver(() => {
    for (const dialog of dialogs) {
      const room = dialog.open ? null : roomOf(dialog);
      if (room) rooms.set(dialog, room);
    }
  });
  for (const dialog of dialogs) sizes.observe(dialog);
  new MutationObserver((changes) => {
    for (const { target } of changes) {
      if (!dialogs.includes(target)) continue;
      if (target.open) keep(target, rooms.get(target));
      else free(target);
    }
  }).observe(document.body, { subtree: true, attributes: true, attributeFilter: ["open"] });
}

// A drawer or dialog drawn open over its page (`data-preview-open`) is what the page looks like
// after a click, and the scrim covers the page as a modal's backdrop does. As with a real modal,
// the page behind takes no pointer, no focus and no reading: it is inert, all but the path to
// each dialog drawn open.
if (document.body.hasAttribute("data-preview-open")) {
  const open = [...document.querySelectorAll("[data-preview-open] > dialog:not([open])")];
  for (const dialog of open) {
    for (let el = dialog; el.parentElement && el !== document.body; el = el.parentElement) {
      for (const other of el.parentElement.children) {
        if (other !== el && !open.some((d) => other.contains(d))) other.inert = true;
      }
    }
  }
}

// ---- Galleries and replays ----

// A searchable gallery: someone types, the cells that do not match go away. The
// cells are plain markup, so without this script the gallery still shows them all.
for (const form of document.querySelectorAll("form[data-preview-search]")) {
  const input = form.querySelector("input");
  const count = form.querySelector("[data-preview-count]");
  const cells = [...document.querySelectorAll("[data-preview-cell][data-terms]")];
  const empty = document.querySelector("[data-preview-empty]");
  const noun = form.dataset.noun ?? "results";
  const total = cells.length;
  let pending = 0;
  const filter = () => {
    const words = input.value.toLowerCase().split(/\s+/).filter(Boolean);
    let shown = 0;
    for (const cell of cells) {
      const terms = cell.dataset.terms;
      const match = words.every((word) => terms.includes(word));
      if (cell.hidden === match) cell.hidden = !match;
      if (match) shown += 1;
    }
    count.textContent = words.length ? `${shown} of ${total} ${noun}` : `${total} ${noun}`;
    if (empty) empty.hidden = shown > 0;
    pending = 0;
  };
  // One pass per frame, however fast the typing.
  input.addEventListener("input", () => {
    if (!pending) pending = requestAnimationFrame(filter);
  });
  form.addEventListener("submit", (event) => event.preventDefault());
}

// Live parts, replayed: what a stream would bring was drawn by the app's own Rust
// into a template, and this plays it one piece at a time. Each piece names the
// list it joins (`data-preview-into`, or the template's `data-preview-replay`),
// whether it goes at the end (`data-preview-at="end"`) rather than the top, and
// an element it takes the place of (`data-preview-replaces`), such as a card that
// moves or a count that changes. A static page has no server to stream from; in an
// app the server sends them.
for (const template of document.querySelectorAll("template[data-preview-replay]")) {
  const pieces = [...template.content.children];
  let at = 0;
  const next = () => {
    const piece = pieces[at++];
    if (!piece) return;
    const into = document.getElementById(piece.dataset.previewInto || template.dataset.previewReplay || "");
    const old = piece.dataset.previewReplaces ? document.getElementById(piece.dataset.previewReplaces) : null;
    if (into) {
      piece.setAttribute("data-preview-new", "");
      if (old && old.parentElement === into && piece.dataset.previewAt !== "end") old.replaceWith(piece);
      else {
        old?.remove();
        if (piece.dataset.previewAt === "end") into.append(piece);
        else into.prepend(piece);
      }
    }
    setTimeout(next, 2800);
  };
  setTimeout(next, 1600);
}
