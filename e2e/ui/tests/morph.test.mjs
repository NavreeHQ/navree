// Morph properties, in real browsers: random trees and random edits, with the
// runtime's own module.
//
// For every seed: the DOM ends equal to the target; a node with an id that
// survives is the same object; what the user typed stays; focus, caret and
// selection stay. Composition is driven through the browser's real input
// protocol (Chromium only: other engines expose no way to start one).

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, join } from "node:path";
import { after, before, test } from "node:test";
import { fileURLToPath } from "node:url";
import { chromium, firefox } from "playwright";

const here = dirname(fileURLToPath(import.meta.url));
const runtime = join(here, "../../../crates/navree-ui/runtime/src");
const ROUNDS = Number(process.env.MORPH_ROUNDS || 3000);

let server;
let origin;

before(async () => {
  const modules = { "/morph.js": await readFile(join(runtime, "morph.js")), "/dom.js": await readFile(join(runtime, "dom.js")) };
  server = createServer((req, res) => {
    if (modules[req.url]) {
      res.writeHead(200, { "content-type": "text/javascript" });
      res.end(modules[req.url]);
    } else {
      res.writeHead(200, { "content-type": "text/html" });
      res.end(`<!doctype html><title>morph</title><body><div id="root"></div>`);
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  origin = `http://127.0.0.1:${server.address().port}`;
});

after(() => server.close());

/** Runs in the page: `rounds` random morphs from `firstSeed`; failures come back. */
async function properties(firstSeed, rounds) {
  const { morph } = await import("/morph.js");

  function random(seed) {
    let state = seed >>> 0 || 1;
    const next = () => {
      state ^= state << 13;
      state ^= state >>> 17;
      state ^= state << 5;
      state >>>= 0;
      return state / 2 ** 32;
    };
    return { next, int: (n) => Math.floor(next() * n), pick: (list) => list[Math.floor(next() * list.length)], chance: (p) => next() < p };
  }

  const TAGS = ["div", "span", "p", "ul", "li", "section", "button", "label"];
  const VOID = ["input", "textarea", "select", "details"];

  /** A plain description of a tree, so it can be edited and built twice. */
  function tree(r, depth, ids) {
    const kind = depth > 3 || r.chance(0.25) ? r.pick(VOID) : r.pick(TAGS);
    const node = { tag: kind, attrs: {}, children: [] };
    if (r.chance(0.45) && ids.length) node.attrs.id = ids.splice(r.int(ids.length), 1)[0];
    if (r.chance(0.3)) node.attrs["data-x"] = String(r.int(5));
    if (r.chance(0.2)) node.attrs.class = r.pick(["a", "b", "a b"]);
    if (kind === "input") {
      node.attrs.type = r.pick(["text", "text", "checkbox"]);
      if (node.attrs.type === "text" && r.chance(0.5)) node.attrs.value = "v" + r.int(4);
      node.attrs.name = "n" + r.int(9);
    } else if (kind === "textarea") {
      node.text = "t" + r.int(4);
    } else if (kind === "select") {
      for (const value of ["1", "2", "3"]) node.children.push({ tag: "option", attrs: { value, ...(r.chance(0.3) ? { selected: "" } : {}) }, children: [], text: value });
    } else if (kind === "details") {
      node.children.push({ tag: "summary", attrs: {}, children: [], text: "s" });
    } else {
      const count = depth > 3 ? 0 : r.int(4);
      for (let i = 0; i < count; i++) node.children.push(r.chance(0.2) ? { text: "x" + r.int(5) } : tree(r, depth + 1, ids));
    }
    return node;
  }

  function collectIds(node, out = []) {
    if (node.attrs?.id) out.push(node.attrs.id);
    for (const child of node.children || []) collectIds(child, out);
    return out;
  }

  /** A second tree over the same ids: edits, reorderings, nodes moved between parents. */
  function edit(r, node) {
    const flat = [];
    (function walk(n, parent) {
      if (n.children) {
        for (const c of n.children) {
          flat.push([c, n]);
          walk(c, n);
        }
      }
    })(node);
    const copy = (n) => JSON.parse(JSON.stringify(n));
    const result = copy(node);
    const edits = 1 + r.int(5);
    for (let i = 0; i < edits; i++) {
      const parents = [];
      (function walk(n) {
        if (n.tag && !["input", "textarea", "select", "details", "option", "summary"].includes(n.tag)) parents.push(n);
        for (const c of n.children || []) walk(c);
      })(result);
      const parent = r.pick(parents);
      switch (r.int(6)) {
        case 0: // shuffle
          for (let j = parent.children.length - 1; j > 0; j--) {
            const k = r.int(j + 1);
            [parent.children[j], parent.children[k]] = [parent.children[k], parent.children[j]];
          }
          break;
        case 1: // remove
          if (parent.children.length) parent.children.splice(r.int(parent.children.length), 1);
          break;
        case 2: // add
          parent.children.splice(r.int(parent.children.length + 1), 0, tree(r, 3, []));
          break;
        case 3: // attribute
          parent.attrs["data-x"] = String(r.int(5));
          if (r.chance(0.3)) delete parent.attrs.class;
          break;
        case 4: { // move a child to another parent, unless that would make a cycle
          if (!parent.children.length) break;
          const [moved] = parent.children.splice(r.int(parent.children.length), 1);
          const candidates = parents.filter((p) => {
            let inside = false;
            (function walk(n) {
              if (n === p) inside = true;
              for (const c of n.children || []) walk(c);
            })(moved);
            return !inside;
          });
          const target = r.pick(candidates.length ? candidates : [parent]);
          target.children.splice(r.int(target.children.length + 1), 0, moved);
          break;
        }
        default: // text
          parent.children.push({ text: "y" + r.int(5) });
      }
    }
    return result;
  }

  function build(node) {
    if (node.tag === undefined) return document.createTextNode(node.text);
    const el = document.createElement(node.tag);
    for (const name in node.attrs) el.setAttribute(name, node.attrs[name]);
    if (node.text !== undefined) el.textContent = node.text;
    for (const child of node.children) el.append(build(child));
    return el;
  }

  const failures = [];
  const root = document.getElementById("root");
  for (let seed = firstSeed; seed < firstSeed + rounds; seed++) {
    const r = random(seed * 2654435761);
    const pool = Array.from({ length: 12 }, (_, i) => `k${i}`);
    const a = tree(r, 0, pool);
    a.tag = "div";
    if (!a.children) a.children = [];
    const b = edit(r, a);
    b.tag = "div";
    root.replaceChildren(build(a));
    const from = root.firstElementChild;
    const before = new Map([...from.querySelectorAll("[id]")].map((el) => [el.id, el]));
    const expected = build(b);
    const target = build(b);
    try {
      morph(from, target);
    } catch (error) {
      failures.push({ seed, what: "threw", error: String(error), stack: error.stack });
      continue;
    }
    if (root.firstElementChild !== from) failures.push({ seed, what: "root replaced" });
    if (!from.isEqualNode(expected)) {
      failures.push({ seed, what: "not equal", got: from.outerHTML, want: expected.outerHTML });
      continue;
    }
    for (const id of collectIds(b)) {
      const old = before.get(id);
      const now = from.querySelector(`[id="${id}"]`);
      if (old && now && old.nodeName === now.nodeName && old !== now) {
        // Identity is promised unless the old node would have had to hold its own new parent.
        failures.push({ seed, what: "identity lost", id, got: from.outerHTML });
        break;
      }
    }
  }
  return failures.slice(0, 5);
}

/** Runs in the page: focus, caret, selection and typed values through morphs. */
async function focusProperties() {
  const { morph, parse } = await import("/morph.js");
  const root = document.getElementById("root");
  const out = {};

  // A caret in an input, its siblings reordering around it.
  root.innerHTML = `<form id="f"><div id="a">A</div><input id="i" name="i" value="server"><div id="b">B</div></form>`;
  const input = document.getElementById("i");
  input.focus();
  input.value = "typed text";
  input.setSelectionRange(2, 7, "forward");
  morph(root.firstElementChild, parse(`<form id="f"><div id="b">B2</div><div id="c">C</div><input id="i" name="i" value="other"><div id="a">A2</div></form>`));
  out.kept = {
    same: document.getElementById("i") === input,
    focused: document.activeElement === input,
    value: input.value,
    start: input.selectionStart,
    end: input.selectionEnd,
    direction: input.selectionDirection,
    order: [...root.firstElementChild.children].map((el) => el.id).join(),
  };

  // The focused input moved to another parent.
  root.innerHTML = `<div id="p1"><input id="m" value="x"></div><div id="p2"></div>`;
  const wrap = document.createElement("div");
  wrap.append(...root.childNodes);
  root.append(wrap);
  const moved = document.getElementById("m");
  moved.focus();
  moved.value = "moved value";
  moved.setSelectionRange(1, 3);
  morph(wrap, parse(`<div><div id="p1"></div><div id="p2"><input id="m" value="y"></div></div>`));
  out.moved = {
    same: document.getElementById("m") === moved,
    parent: moved.parentElement.id,
    focused: document.activeElement === moved,
    value: moved.value,
    start: moved.selectionStart,
    end: moved.selectionEnd,
  };

  // Typed values in fields that are not focused, and checkboxes and selects the user changed.
  root.innerHTML = `<form id="v"><input id="t" value="a"><textarea id="ta">a</textarea><input id="c" type="checkbox"><select id="s"><option value="1">1</option><option value="2">2</option></select><input id="fresh" value="a"></form>`;
  document.getElementById("t").value = "user";
  document.getElementById("ta").value = "user";
  document.getElementById("c").checked = true;
  document.getElementById("s").value = "2";
  morph(root.firstElementChild, parse(`<form id="v"><input id="t" value="b"><textarea id="ta">b</textarea><input id="c" type="checkbox"><select id="s"><option value="1">1</option><option value="2">2</option></select><input id="fresh" value="b"></form>`));
  out.values = {
    input: document.getElementById("t").value,
    textarea: document.getElementById("ta").value,
    checked: document.getElementById("c").checked,
    select: document.getElementById("s").value,
    untouched: document.getElementById("fresh").value,
  };

  // Open state the browser holds stays open; dialog too.
  root.innerHTML = `<div id="o"><details id="d"><summary>s</summary>x</details><dialog id="dl">y</dialog></div>`;
  document.getElementById("d").open = true;
  document.getElementById("dl").showModal();
  const dialog = document.getElementById("dl");
  morph(root.firstElementChild, parse(`<div id="o"><details id="d"><summary>s</summary>x2</details><dialog id="dl">y2</dialog></div>`));
  out.open = { details: document.getElementById("d").open, dialog: dialog.open, same: document.getElementById("dl") === dialog, modal: dialog.matches(":modal") };
  dialog.close();

  // Keep and persist.
  root.innerHTML = `<div id="w"><div id="island" data-ui-keep>mine</div><div id="toaster">t</div><div id="stored" data-ui-persist>s</div></div>`;
  (await import("/dom.js")).persistent.add(document.getElementById("toaster"));
  morph(root.firstElementChild, parse(`<div id="w"><div id="island" data-ui-keep>theirs</div></div>`));
  out.keep = { island: document.getElementById("island").textContent, persisted: !!document.getElementById("toaster"), asked: !!document.getElementById("stored") };
  return out;
}

for (const [name, engine, options] of [
  ["chromium", chromium, {}],
  ["firefox", firefox, {}],
]) {
  test(`random morphs reach the target and keep identity (${name})`, { timeout: 300000 }, async () => {
    const browser = await engine.launch(options);
    const page = await browser.newPage();
    await page.goto(origin);
    const failures = await page.evaluate(properties.toString().replace(/^async function properties/, "async function") .replace(/^/, "(") + ")(1," + ROUNDS + ")");
    await browser.close();
    assert.deepEqual(failures, []);
  });

  test(`focus, caret, selection and typed values survive (${name})`, { timeout: 60000 }, async () => {
    const browser = await engine.launch(options);
    const page = await browser.newPage();
    await page.goto(origin);
    const out = await page.evaluate(focusProperties);
    await browser.close();
    assert.deepEqual(out.kept, { same: true, focused: true, value: "typed text", start: 2, end: 7, direction: "forward", order: "b,c,i,a" });
    assert.deepEqual(out.moved, { same: true, parent: "p2", focused: true, value: "moved value", start: 1, end: 3 });
    assert.deepEqual(out.values, { input: "user", textarea: "user", checked: true, select: "2", untouched: "b" });
    assert.deepEqual(out.open, { details: true, dialog: true, same: true, modal: true });
    assert.deepEqual(out.keep, { island: "mine", persisted: true, asked: false });
  });
}

test("an IME composition survives a morph (chromium)", { timeout: 60000 }, async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage();
  await page.goto(origin);
  await page.evaluate(async () => {
    const { morph, parse } = await import("/morph.js");
    window.morph = morph;
    window.parse = parse;
    const root = document.getElementById("root");
    root.innerHTML = `<div id="w"><p id="above">one</p><input id="i" value=""></div>`;
    window.events = [];
    const input = document.getElementById("i");
    for (const name of ["compositionstart", "compositionend", "blur"]) input.addEventListener(name, () => window.events.push(name));
    input.focus();
  });
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Input.imeSetComposition", { text: "にほ", selectionStart: 2, selectionEnd: 2 });
  await page.evaluate(() => {
    morph(document.getElementById("w"), parse(`<div id="w"><p id="above">two</p><p id="new">three</p><input id="i" value="server" class="changed"></div>`));
  });
  await cdp.send("Input.imeSetComposition", { text: "にほん", selectionStart: 3, selectionEnd: 3 });
  await cdp.send("Input.insertText", { text: "日本" });
  const state = await page.evaluate(() => ({
    value: document.getElementById("i").value,
    events: window.events,
    above: document.getElementById("above").textContent,
    fresh: !!document.getElementById("new"),
    focused: document.activeElement?.id,
  }));
  await browser.close();
  assert.equal(state.value, "日本");
  assert.deepEqual(state.events, ["compositionstart", "compositionend"]);
  assert.equal(state.above, "two");
  assert.equal(state.fresh, true);
  assert.equal(state.focused, "i");
});

// A form's named fields shadow its own properties (`form.id` is the field called
// "id"). Every name below is one a real form might have, or an attacker might pick.
for (const [name, engine] of [["chromium", chromium], ["firefox", firefox]]) {
  test(`a form whose fields are named like its properties still morphs (${name})`, { timeout: 60000 }, async () => {
    const browser = await engine.launch();
    const page = await browser.newPage();
    await page.goto(origin);
    const out = await page.evaluate(async () => {
      const { morph, parse } = await import("/morph.js");
      const names = ["id", "name", "method", "action", "target", "elements", "children", "childNodes", "attributes", "firstChild", "nextSibling", "parentNode", "nodeName", "nodeType", "getAttribute", "hasAttribute", "setAttribute", "removeAttribute", "insertBefore", "removeChild", "contains", "querySelector", "querySelectorAll", "cloneNode", "moveBefore"];
      const fields = (value) => names.map((n) => `<input name="${n}" value="${value}">`).join("");
      const root = document.getElementById("root");
      root.innerHTML = `<div id="w"><form id="f" method="post" action="/a" data-ui-form>${fields("old")}<p id="note">one</p><button>Go</button></form></div>`;
      const form = document.getElementById("f");
      const all = (el) => [...Element.prototype.querySelectorAll.call(el, "input")];
      const inputs = all(form);
      inputs[3].value = "typed";
      const note = document.getElementById("note");
      let error = null;
      try {
        morph(root.firstElementChild, parse(`<div id="w"><form id="f" method="post" action="/b" data-ui-form class="changed">${fields("new")}<p id="note">two</p><span id="added">x</span><button>Go</button></form></div>`));
      } catch (e) {
        error = String(e);
      }
      const now = document.getElementById("f");
      return {
        error,
        sameForm: now === form,
        sameInputs: all(now).every((input, at) => input === inputs[at]),
        sameNote: document.getElementById("note") === note,
        note: note.textContent,
        added: !!document.getElementById("added"),
        typed: inputs[3].value,
        untouched: inputs[0].value,
        action: Element.prototype.getAttribute.call(now, "action"),
        cls: Element.prototype.getAttribute.call(now, "class"),
      };
    });
    await browser.close();
    assert.deepEqual(out, { error: null, sameForm: true, sameInputs: true, sameNote: true, note: "two", added: true, typed: "typed", untouched: "new", action: "/b", cls: "changed" });
  });
}
