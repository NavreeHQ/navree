// The extensions module: a plugin's own components. The server names a component's script and
// stylesheet on its element, with its props, the routes its script may call, and the bodies it
// draws into (children kept by the morph):
//
//   <div data-slot="geo:map" data-ui-ext="/_navree/ui/ext/geo.map-{hash}.js"
//        data-ui-ext-sheet="/_navree/ui/ext/geo.map-{hash}.css" data-ui-ext-routes="/geo/ =/deliveries/42/drop"
//        data-ui-props="{json}">
//     <div data-slot="geo:map-body" data-ui-keep>…</div>
//   </div>
//
// This links each stylesheet an element names, and imports each script (one this site serves at its
// extension route, and nothing else) to call its `mount(el, props, ui)` once. Before a script is
// imported, each of its element's bodies is sealed, for as long as it lives: every runtime hook inside
// goes, the body is marked stored content (nothing inside drives the runtime, stands in for one of the
// page's ids or mounts an extension), every form field inside joins no form, and an observer keeps it
// so whatever the script or a library it bundles writes there later, and however. What the script is
// given reaches only the routes the server wrote on its element, through core's own write path. On a
// page that starts clean nothing here runs.
import { E, T, all, attr, body, byId, cookie, emit, every, fetchHere, fresh, has, head, holds, make, on, set, stored, unset } from "./dom.js";

/** A UI extension's file: at the extension route, by namespace, name and content hash. */
const FILE = /^\/_navree\/ui\/ext\/[a-z][a-z0-9_]{0,63}\.[a-z0-9-]{1,64}-[0-9a-f]{12}\.(js|css)$/;
/** The form a sealed field names: an id no element has, so it joins none. */
const SEALED = "ui-sealed";
/** What a form takes into its post. */
const FIELDS = "input,select,textarea,button,fieldset,output,object";
const MARK = "data-ui-stored";
const OPS = "application/vnd.navree.ops+json";
/** Read through the prototype: a named element can't stand in for it. */
const up = /** @type {(this: Node) => Element | null} */ (Object.getOwnPropertyDescriptor(Node.prototype, "parentNode")?.get);

/**
 * The path of a file named on an element, when it is an extension's script (`js`) or stylesheet
 * (`css`): on this origin, at the extension route, with nothing after its name (no query, fragment,
 * user name or password).
 * @param {string | null} value @param {string} ext
 */
const address = (value, ext) => {
  try {
    const url = new URL(value || "", location.href);
    return FILE.exec(url.pathname)?.[1] === ext && url.href === location.origin + url.pathname ? url.pathname : null;
  } catch {
    return null;
  }
};

/** @param {string} text */
const parse = (text) => {
  try {
    return JSON.parse(text || "null");
  } catch {
    return null;
  }
};

/** @type {{apply: (envelope: any) => boolean, writing: <R>(write: () => Promise<R>) => Promise<R>, say: (text: string) => void} | null} */
let core = null;
/** Core's write path, op applier and announcer, asked for until core gives them: none of their state is copied here. */
const hand = () => {
  if (!core) {
    const asked = /** @type {any} */ ({});
    emit(new CustomEvent("ui:core", { detail: asked }));
    if (asked.apply && asked.writing) core = asked;
  }
  return core;
};

/** The bodies sealed, each once and for good. @type {WeakSet<Element>} */
const sealed = new WeakSet();

/** A field joins no form. @param {Element} el */
const detach = (el) => E.matches.call(el, FIELDS) && attr(el, "form") !== SEALED && set(el, "form", SEALED);

/**
 * Takes every runtime hook off `el`, and takes it out of any form. What acts by being in a page and no
 * policy stops (a base that repoints every link, a refresh) is taken out, as from a fetched page.
 * @param {Element} el
 */
const scrub = (el) => {
  if (E.matches.call(el, "meta[http-equiv],base")) el.remove();
  for (const name of E.getAttributeNames.call(el)) if (name.startsWith("data-ui-")) unset(el, name);
  detach(el);
};

/**
 * Seals a body before its script is imported: scrubs what it holds, marks it stored content, and
 * keeps both so for whatever is written in it later. A change to an attribute is judged by its name
 * first, so the styles and classes a library moves on every frame cost a comparison.
 * @param {Element} b
 */
function seal(b) {
  if (sealed.has(b)) return;
  sealed.add(b);
  all(b, "*").forEach(scrub);
  set(b, MARK, "");
  new MutationObserver((records) => {
    for (const { target, attributeName: name, addedNodes } of records) {
      if (!name) {
        for (const node of addedNodes) if (node instanceof Element) [node, ...all(node, "*")].forEach(scrub);
      } else if (target !== b) {
        if (name.startsWith("data-ui-")) unset(/** @type {Element} */ (target), name);
        else if (name === "form") detach(/** @type {Element} */ (target));
      } else if (name === MARK) {
        // Its mark comes back, unless it lies in another sealed body: that one's mark covers it.
        if (!has(b, MARK) && !stored(b)) set(b, MARK, "");
      } else if (name.startsWith("data-ui-") && name !== "data-ui-keep") unset(b, name);
    }
  }).observe(b, { subtree: true, childList: true, attributes: true });
}

/**
 * The bodies of `el`: the elements in it the morph keeps, where no other extension, island or kept
 * element lies between.
 * @param {Element} el @returns {Element[]}
 */
const bodiesOf = (el) => [...all(el, "[data-ui-keep]")].filter((b) => E.closest.call(/** @type {Element} */ (up.call(b)), "[data-ui-ext],[data-ui-island],[data-ui-keep]") === el);

/**
 * What a script is given: helpers bound to its element, that end when it leaves.
 * @param {Element} el @param {string} src @param {AbortSignal} signal @param {Element[]} bodies
 */
function helpers(el, src, signal, bodies) {
  const namespace = /** @type {string} */ (src.split("/").pop()).split(".")[0];
  /** Whether the script may reach `node`: not stored content, or in one of its own bodies. @param {Element} node */
  const ours = (node) => !stored(node) || bodies.includes(/** @type {Element} */ (E.closest.call(node, "[data-ui-stored],[data-ui-own]")));
  /**
   * The address `url` names, when this element's script may reach it: this origin, no user name or
   * password, and a path, as parsed, under one of the route prefixes the server wrote on the element, or
   * equal to one of its exact addresses. They are read from the element as the call is made; a string in
   * the props opens nothing.
   * @param {string} url
   */
  const allowed = (url) => {
    const to = new URL(url, location.href), path = to.pathname;
    if (to.origin === location.origin && !to.username && !to.password && (attr(el, "data-ui-ext-routes") || "").split(" ").some((r) => (r[0] === "=" ? r.slice(1) === path : r[0] === "/" && path.startsWith(r)))) return to.href;
    throw new TypeError("not a route of this extension");
  };
  /** @param {string} tone @param {string} text */
  const toast = (tone, text) => emit(new CustomEvent("ui:toast", { detail: { tone, text } }));
  return Object.freeze({
    contract: 1,
    namespace,
    signal,
    /** @param {string} selector */
    find: (selector) => [...all(el, selector)].find(ours) || null,
    /** @param {string} selector */
    findAll: (selector) => [...all(el, selector)].filter(ours),
    /** From `node` up to the element: an event's target may be the window, which is none of it. @param {Node} node @param {string} selector */
    closest(node, selector) {
      for (let at = node instanceof Node && holds(el, node) ? node : null; at; at = at === el ? null : up.call(at)) if (at instanceof Element && E.matches.call(at, selector) && ours(at)) return at;
      return null;
    },
    /** @param {Node} node */
    stored: (node) => !ours(/** @type {Element} */ (node instanceof Element ? node : up.call(node))),
    /** @param {EventTarget} target @param {string} type @param {(event: Event) => void} listener @param {AddEventListenerOptions | boolean} [options] */
    on(target, type, listener, options) {
      const given = { ...(typeof options === "object" ? options : { capture: !!options }) };
      given.signal = given.signal ? AbortSignal.any([given.signal, signal]) : signal;
      on(target, type, listener, given);
      return () => T.removeEventListener.call(target, type, listener, given);
    },
    /** @param {number} ms @param {() => void} run */
    later(ms, run) {
      const timer = setTimeout(() => signal.aborted || run(), ms);
      return () => clearTimeout(timer);
    },
    /**
     * A write to one of the element's routes, through core's write path with the session's token. Ops
     * are applied; JSON is the script's; a page is shown only when asked for. Anything else fails.
     * @param {string} url @param {FormData | URLSearchParams | Record<string, string>} [data]
     * @param {{method?: string, quiet?: boolean, page?: boolean}} [options]
     */
    async call(url, data, options = {}) {
      let ok = false, status = 0, answer = null;
      try {
        const href = allowed(url), given = /** @type {any} */ (hand());
        /** @type {Record<string, string>} */
        const headers = { "x-navree-ui": "1", "x-navree-from": location.pathname + location.search, accept: OPS + ", application/json, text/html" };
        const token = cookie("XSRF-TOKEN");
        if (token) headers["x-xsrf-token"] = token;
        const sent = data instanceof FormData || data instanceof URLSearchParams ? data : new URLSearchParams(Object.entries(data || {}));
        // A write takes the page's turn: what was fetched before it is not shown after it.
        const response = await given.writing(() => fetchHere(href, { method: options.method || "POST", headers, body: sent }));
        status = response.status;
        const type = response.headers.get("content-type") || "";
        // A page that starts clean answers nothing into this one.
        if (!response.headers.has("x-navree-fresh")) {
          // The ops bring their own word on how it went.
          if (type.startsWith(OPS)) {
            given.apply(await response.json());
            return { ok: response.ok, status, data: null };
          }
          if (type.startsWith("text/html")) {
            if (options.page) location.assign(response.url);
            ok = !!options.page && response.ok;
          } else {
            if (type.startsWith("application/json")) answer = await response.json();
            ok = response.ok;
          }
        }
      } catch {}
      if (!ok && !options.quiet) toast("danger", "That didn't work. Try again.");
      return { ok, status, data: answer };
    },
    /**
     * A read of one of the element's routes. Headers of the runtime's own are not the script's to send.
     * @param {string} url @param {Record<string, string>} [headers]
     */
    async get(url, headers = {}) {
      /** @type {Record<string, string>} */
      const sent = { "x-navree-ui": "1" };
      for (const name in headers) if (!/^x-navree-/i.test(name)) sent[name] = headers[name];
      const response = await fetchHere(allowed(url), { headers: sent });
      if (response.headers.has("x-navree-fresh")) throw new TypeError("an answer of a page that starts clean");
      return response;
    },
    /** @param {string} name @param {unknown} [detail] */
    emit: (name, detail) => T.dispatchEvent.call(el, new CustomEvent(namespace + ":" + name, { bubbles: true, cancelable: true, detail })),
    toast,
    /** @param {string} text */
    announce: (text) => hand()?.say(text),
    /** @param {string} id */
    refresh(id) {
      const region = byId(id);
      if (region) T.dispatchEvent.call(region, new CustomEvent("ui:refresh", { bubbles: true }));
    },
  });
}

/**
 * Every element mounted or mounting: the address it was mounted from, its props as written, whether its
 * script has answered, and what it gave back.
 * @typedef {{at: string | null, props: string, ready: boolean, stop: AbortController, update: ((props: unknown) => void) | null, done: (() => void) | null}} Mounted
 * @type {Map<Element, Mounted>}
 */
const mounted = new Map();
/** The mounted elements a morph reached since the last scan: each is given its props again. @type {Set<Element>} */
const touched = new Set();
/** Whether the page was swapped since the last scan. */
let paged = false;

/**
 * Gives a mounted element its props as they are now: to its `update`, or, when they changed and it has
 * none, by mounting it again.
 * @param {Element} el @param {Mounted} m @param {boolean} changed
 */
function renew(el, m, changed) {
  if (!m.ready) return;
  if (m.update) {
    try {
      m.update(parse(m.props));
    } catch {}
  } else if (changed) {
    leave(el);
    mount(el, m.at, m.props);
  }
}

/** Mounts `el` with the script `at` names, if it names one. @param {Element} el @param {string | null} at @param {string} props */
async function mount(el, at, props) {
  const src = address(at, "js");
  if (!src) return;
  const stop = new AbortController(), bodies = bodiesOf(el);
  /** @type {Mounted} */
  const m = { at, props, ready: false, stop, update: null, done: null };
  mounted.set(el, m);
  // Before anything of the script's runs, and for good: a script that fails leaves its bodies sealed.
  bodies.forEach(seal);
  unset(el, "data-ui-failed");
  let why = "could not be loaded";
  try {
    const module = await import(location.origin + src);
    // It left, or now names another script, while its script was on its way.
    if (mounted.get(el) !== m) return;
    why = module.contract !== 1 ? `is written for contract ${module.contract}; this page runs contract 1` : typeof module.mount !== "function" ? "has no mount" : "";
    if (why) throw why;
    why = "failed to mount";
    const given = m.props;
    const back = await module.mount(el, parse(given), helpers(el, src, stop.signal, bodies));
    const done = typeof back === "function" ? back : typeof back?.leave === "function" ? () => back.leave() : null;
    if (mounted.get(el) !== m) return done?.();
    m.done = done;
    m.update = typeof back?.update === "function" ? (next) => back.update(next) : null;
    m.ready = true;
    set(el, "data-ui-mounted", "");
    // Props that came while it mounted.
    if (m.props !== given) renew(el, m, true);
  } catch {
    if (mounted.get(el) === m) {
      set(el, "data-ui-failed", "");
      console.error(`navree-ui: ${src} ${why}`);
    }
  }
}

/** Lets `el` go: its signal ends what it listens to and its timers, and its script undoes what it did. @param {Element} el */
function leave(el) {
  const m = /** @type {Mounted} */ (mounted.get(el));
  mounted.delete(el);
  unset(el, "data-ui-mounted");
  m.stop.abort();
  try {
    m.done?.();
  } catch {}
}

/**
 * Mounts what is new, gives props to what has new ones or was morphed, and lets go of what left the page,
 * went into stored content, or names another script. Stored content and sealed bodies are never mounted:
 * `every` leaves them out.
 */
function scan() {
  for (const [el, m] of mounted) if (!holds(document, el) || stored(el) || attr(el, "data-ui-ext") !== m.at) leave(el);
  for (const el of every("[data-ui-ext]:not([data-ui-island])")) {
    const m = mounted.get(el), props = attr(el, "data-ui-props") || "";
    if (!m) mount(el, attr(el, "data-ui-ext"), props);
    else if (m.props !== props || touched.has(el)) {
      const changed = m.props !== props;
      m.props = props;
      renew(el, m, changed);
    }
  }
  touched.clear();
}

/** The stylesheet links of extensions in the head: the server's and this module's. */
const links = () => [...all(head(), "link[data-ui-ext-link]")];
/** The stylesheets the page's elements name, stored content left out. */
const named = () => every("[data-ui-ext-sheet]").map((el) => address(attr(el, "data-ui-ext-sheet"), "css")).filter(Boolean);

/** Links each stylesheet an element names that the head lacks, once. */
function link() {
  const have = links().map((l) => address(attr(l, "href"), "css"));
  for (const href of named()) {
    if (have.includes(href)) continue;
    have.push(href);
    const l = make("link");
    set(l, "rel", "stylesheet");
    set(l, "href", /** @type {string} */ (href));
    set(l, "data-ui-ext-link", "");
    head().append(l);
  }
}

/** Takes out the extension stylesheets no element of the page names: after a page swap. */
function drop() {
  const wanted = named();
  for (const l of links()) if (!wanted.includes(address(attr(l, "href"), "css"))) l.remove();
}

if (!fresh()) {
  link();
  scan();
  on(document, "ui:before-swap", (e) => {
    const from = /** @type {CustomEvent} */ (e).detail;
    if (from === body()) paged = true;
    for (const el of mounted.keys()) if (holds(from, el)) touched.add(el);
  });
  on(document, "ui:swap", () => {
    if (paged) drop();
    paged = false;
    link();
    scan();
  });
}
