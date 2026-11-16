// A branch of a tree fetches its children as it opens. An answer of a page that starts clean (it carries
// `x-navree-fresh`) is never drawn into a plain page: the branch shows that its children could not come.
import { test } from "node:test";
import assert from "node:assert/strict";
import { answer, install, settle } from "./fake-dom.mjs";

const page = install({
  url: "https://app.test/zones",
  html: `<!doctype html><html data-build="b1"><head></head><body>
    <main id="main">
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

await import("../src/tree.js");
await settle();

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
/** A branch's children, as the server answers them. @param {Record<string, string>} headers */
const children = (headers) =>
  answer(
    `<ul data-slot="tree-group">
      <li data-slot="tree-item" data-level="2" data-setsize="2" data-posinset="1"><div data-slot="tree-row"><span data-slot="tree-label"><span data-slot="tree-text">Model Town</span></span></div></li>
      <li data-slot="tree-item" data-level="2" data-setsize="2" data-posinset="2"><div data-slot="tree-row"><span data-slot="tree-label"><span data-slot="tree-text">Sarabha Nagar</span></span></div></li>
    </ul>`,
    { headers: { "content-type": "text/html; charset=utf-8", ...headers } },
  );
const text = () => $("zones").textContent;

test("a fresh answer is refused", async () => {
  page.answers.push(children({ "x-navree-fresh": "1" }));
  $("open-ludhiana").click();
  await settle();
  assert.deepEqual(page.fetched.map((f) => new URL(f.url).pathname), ["/zones/children"]);
  assert.equal(text().includes("Model Town"), false, "children fetched from a page that must start clean were drawn into a plain page");
  assert.equal(text().includes("Sarabha Nagar"), false, "children fetched from a page that must start clean were drawn into a plain page");
  // The group says its children could not come, and offers to ask again.
  const note = $("ludhiana-group").querySelector("li[data-slot=tree-note]");
  assert.ok(note, "the group shows no failed note");
  assert.equal(note.querySelector("[data-slot=tree-row]").hasAttribute("data-retry"), true);
  assert.equal(text().includes("Couldn't load this."), true);

  // Asked again and answered by a plain page, the same branch takes its children.
  page.answers.push(children({}));
  note.querySelector("[data-slot=tree-retry]").click();
  await settle();
  assert.equal(text().includes("Model Town"), true);
  assert.equal(text().includes("Sarabha Nagar"), true);
});
