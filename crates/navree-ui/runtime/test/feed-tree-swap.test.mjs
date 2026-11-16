// A feed that adds or drops rows, and a tree that draws a branch's children, say so with `ui:swap`, as the lazy
// list does: a component of a plugin's in a row or an item mounts as it arrives, and leaves with its row.
import { test } from "node:test";
import assert from "node:assert/strict";
import { register } from "node:module";
import { answer, fire, install, settle } from "./fake-dom.mjs";

register("./ext-loader.mjs", import.meta.url);

const EXT = "/_navree/ui/ext";
const page = install({
  url: "https://app.test/activity",
  observe: true,
  html: `<!doctype html><html data-build="b1"><head></head><body>
    <main id="main">
      <div id="recent" data-ui-region data-ui-src="/activity" data-ui-feed="recent-rows" data-ui-feed-max="1"><ul id="recent-rows"></ul></div>
      <div id="zones" data-ui-tree data-select="none" data-label="Zones">
        <ul data-slot="tree-root">
          <li data-slot="tree-item" data-level="1" data-setsize="1" data-posinset="1" data-branch data-lazy>
            <div data-slot="tree-row"><span id="open-ludhiana" data-slot="tree-toggle" aria-hidden="true"></span><span data-slot="tree-label"><span data-slot="tree-text">Ludhiana</span></span></div>
            <ul id="ludhiana-group" data-slot="tree-group" data-src="/zones/children"></ul>
          </li>
        </ul>
        <div data-slot="tree-status" role="status" aria-live="polite"></div>
      </div>
    </main>
  </body></html>`,
});

// The fake document keeps what is written to a template out of its content, where a feed reads its rows.
const written = /** @type {PropertyDescriptor} */ (Object.getOwnPropertyDescriptor(Element.prototype, "innerHTML"));
Object.defineProperty(HTMLTemplateElement.prototype, "innerHTML", {
  configurable: true,
  get: written.get,
  set(/** @type {string} */ text) {
    const holder = document.createElement("div");
    written.set?.call(holder, text);
    /** @type {any} */ (this).content.replaceChildren(...holder.childNodes);
  },
});

await import("../src/core.js");
await import("../src/tree.js");
await import("../src/extensions.js");
await settle();

const g = /** @type {any} */ (globalThis);
const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
/** @param {() => boolean} done @param {string} what */
async function until(done, what) {
  for (let i = 0; i < 400; i++) {
    if (done()) return;
    await new Promise((go) => setTimeout(go, 5));
  }
  assert.fail(`still waiting for ${what}`);
}
/** The calls the script of the element `id` got, in order. @param {string} id */
const calls = (id) => (g.seen || []).filter((/** @type {any} */ s) => s.id === id).map((/** @type {any} */ s) => s.call);
/** A component of a plugin's, as a row or an item carries one. @param {string} id */
const pad = (id) => `<span id="${id}" data-slot="proof:pad" data-ui-ext="${EXT}/proof.pad-0123456789ab.js" data-ui-props='{"step":1}'></span>`;
const ROWS = { headers: { "content-type": "text/html; charset=utf-8" } };

test("a component in a row a feed adds mounts, and leaves when the feed drops its row", async () => {
  page.answers.push(answer(`<li id="r1">${pad("in-r1")} Harpreet Sandhu signed in</li>`, ROWS));
  fire($("recent"), "ui:refresh");
  await until(() => $("in-r1")?.hasAttribute("data-ui-mounted"), "the component in the first row to mount");

  // The next row comes, and the feed keeps one: the first row goes, and its component leaves.
  page.answers.push(answer(`<li id="r2">${pad("in-r2")} Meera Sharma signed in</li>`, ROWS));
  fire($("recent"), "ui:refresh");
  await until(() => $("in-r2")?.hasAttribute("data-ui-mounted"), "the component in the second row to mount");
  assert.equal($("r1"), null);
  assert.deepEqual(calls("in-r1"), ["mount", "leave"]);
  assert.equal(g.uis["in-r1"].signal.aborted, true, "a component in a dropped row still listens");
});

test("a component in a tree item drawn as its branch opens mounts", async () => {
  page.answers.push(
    answer(
      `<ul data-slot="tree-group"><li data-slot="tree-item" data-level="2" data-setsize="1" data-posinset="1"><div data-slot="tree-row"><span data-slot="tree-label"><span data-slot="tree-text">Model Town</span>${pad("in-item")}</span></div></li></ul>`,
      ROWS,
    ),
  );
  $("open-ludhiana").click();
  await until(() => $("in-item")?.hasAttribute("data-ui-mounted"), "the component in the tree item to mount");
  assert.deepEqual(calls("in-item"), ["mount"]);
});
