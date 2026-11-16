// Islands: a part of a server-drawn page that a script of the app's own draws. The
// server writes the island's script address and its props on the island, and what
// shows before the script runs (and with no script at all) in its body:
//
//   <div data-slot="island" data-ui-island="/_navree/ui/island/{name}-{hash}.js" data-ui-props="{json}">
//     <div data-slot="island-body" data-ui-keep>…</div>
//   </div>
//
// A plugin's script is named under its namespace: `{namespace}.{name}-{hash}.js`. Its
// island's body is sealed before the script is imported, as a plugin component's owned body
// is: the extensions module, which the server loads ahead of this one on such a page, seals
// it when asked. A body no one seals gets no plugin script: the island is marked failed.
//
// This imports the script, one of the app's own and nothing else, and calls its
// `mount(body, props, island)` once. A morph never touches the body (it is kept), so
// what the script drew stays. What `mount` returns (or resolves to) says the rest: a
// function is called when the island leaves the page; an object's `update(props)` takes
// new props from the server in place and its `leave()` is called when it leaves. Without
// `update`, new props mount the island again. Props are JSON: data, never markup or code.
import { attr, every, fresh, on } from "./dom.js";

/**
 * An island script's address: this origin, under the mount, named by its content hash, with
 * a plugin's namespace and a dot before the name when the script is a plugin's.
 */
const SCRIPT = /^\/_navree\/ui\/island\/([a-z][a-z0-9_]{0,63}\.)?[a-z0-9-]{1,64}-[0-9a-f]{12}\.js$/;

/**
 * Every island the page has mounted or is mounting: its script, its props as written,
 * whether its script has answered, and what it gave back.
 * @typedef {{src: string, props: string, ready: boolean, update: ((props: unknown) => void) | null, leave: (() => void) | null}} Island
 * @type {Map<Element, Island>}
 */
const islands = new Map();

/**
 * The address to import for `src`, when it is one of the app's island scripts. A page that starts
 * clean imports the app's own and no plugin's. @param {string} src
 */
function script(src) {
  try {
    const url = new URL(src, location.href);
    const named = SCRIPT.exec(url.pathname);
    return url.origin === location.origin && named && !(named[1] && fresh()) && !url.search && !url.hash ? url.href : null;
  } catch {
    return null;
  }
}

/** The props, parsed: anything that isn't JSON is none. @param {string} text */
function propsOf(text) {
  try {
    return JSON.parse(text || "null");
  } catch {
    return null;
  }
}

/**
 * Where the island `el` with the script at `src` is drawn: its body, or the island itself when it
 * has none. A plugin's script (a namespace before its name) is drawn only in a body the extensions
 * module sealed when asked: the ask is an event sent to the body, which that module cancels once
 * the body is sealed. With no seal, the island is marked failed and let go of, so the next scan
 * asks again; the extensions module has this module scan when it comes.
 * @param {Element} el @param {string} src
 */
function body(el, src) {
  const b = el.querySelector(":scope > [data-slot=island-body]");
  if (!/\.[^/]+\.js$/.test(src) || (b && !b.dispatchEvent(new Event("ui:seal", { cancelable: true })))) return b || el;
  islands.delete(el);
  el.setAttribute("data-ui-failed", "");
  throw 0;
}

/** Mounts the island `el` with the script at `src`. @param {Element} el @param {string} src @param {string} props */
async function mount(el, src, props) {
  /** @type {Island} */
  const state = { src, props, ready: false, update: null, leave: null };
  islands.set(el, state);
  el.removeAttribute("data-ui-failed");
  try {
    // A plugin's island body is sealed before anything of its script runs.
    body(el, src);
    const module = await import(src);
    // It left, or now names another script, while its script was on its way.
    if (islands.get(el) !== state) return;
    // Props that came while the script loaded are the ones it mounts with.
    const given = await module.mount(body(el, src), propsOf(state.props), el);
    const leave = typeof given === "function" ? given : typeof given?.leave === "function" ? () => given.leave() : null;
    if (islands.get(el) !== state) {
      leave?.();
      return;
    }
    state.ready = true;
    state.leave = leave;
    state.update = typeof given?.update === "function" ? (next) => given.update(next) : null;
    el.setAttribute("data-ui-mounted", "");
  } catch {
    // The island's script failed: the body shows what the server drew, and the page goes on.
    if (islands.get(el) === state) el.setAttribute("data-ui-failed", "");
  }
}

/** Lets the island `el` go: its script undoes what it did. @param {Element} el */
function leave(el) {
  const state = islands.get(el);
  islands.delete(el);
  el.removeAttribute("data-ui-mounted");
  try {
    state?.leave?.();
  } catch {}
}

/** Mounts what is new, gives new props to what has them, and lets go of what left. */
function scan() {
  for (const el of [...islands.keys()]) if (!el.isConnected) leave(el);
  for (const el of every("[data-ui-island]")) {
    const src = script(attr(el, "data-ui-island") || "");
    const props = attr(el, "data-ui-props") || "";
    const state = islands.get(el);
    if (state && state.src === src) {
      if (state.props === props) continue;
      state.props = props;
      // Still loading: it mounts with these.
      if (!state.ready) continue;
      if (state.update) {
        try {
          state.update(propsOf(props));
        } catch {}
        continue;
      }
    }
    if (state) leave(el);
    if (src) mount(el, src, props);
  }
}

scan();
on(document, "ui:swap", scan);
// The extensions module has come: a plugin's island that waited for its seal is mounted now.
on(document, "ui:sealer", scan);
