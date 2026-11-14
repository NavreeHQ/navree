// Every fetch of the runtime goes to this site only, and an answer that ended on another site is a
// failure: a page loads for real, a region keeps what it had, an upload says it failed.
import { test } from "node:test";
import assert from "node:assert/strict";
import { answer, install, settle } from "./fake-dom.mjs";

const page = install({
  url: "https://app.test/deals",
  cookie: "XSRF-TOKEN=tok",
  html: `<!doctype html><html data-build="b1"><head></head><body data-ui-boost>
    <main id="main"><a id="next" href="/deals/2">Next</a></main>
    <div id="far" data-ui-region data-ui-src="https://evil.example/kpis">kept</div>
    <div id="up" data-ui-upload data-send="https://evil.example/files" data-l='{"offline":"offline"}'>
      <input type="file" name="file">
      <ul data-slot="upload-list"></ul><p data-slot="upload-status"></p>
      <template><li data-slot="upload-item"><span data-slot="attachment-title"></span><span data-slot="attachment-description"></span><progress></progress></li></template>
    </div>
  </body></html>`,
});

const opened = /** @type {string[]} */ ([]);
Object.defineProperty(globalThis, "XMLHttpRequest", {
  configurable: true,
  writable: true,
  value: class {
    upload = {};
    /** @param {string} _ @param {string} url */
    open(_, url) {
      opened.push(url);
    }
    setRequestHeader() {}
    send() {}
    abort() {}
  },
});

const dom = await import("../src/dom.js");
await import("../src/core.js");
await import("../src/upload.js");
await settle();

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));

test("an address on another site is refused before anything is sent", async () => {
  page.fetched.length = 0;
  await assert.rejects(dom.fetchHere("https://evil.example/x", { headers: { "x-xsrf-token": "tok" } }));
  assert.equal(page.fetched.length, 0);
});

test("an answer that ended on another site is a failure", async () => {
  page.answers.push(answer("<p>x</p>", { url: "https://evil.example/x", redirected: true }));
  await assert.rejects(dom.fetchHere("/x"));
  page.answers.push(answer("<p>x</p>"));
  const ok = await dom.fetchHere("/y");
  assert.equal(ok.status, 200);
});

test("a region whose source is on another site is never fetched", async () => {
  page.fetched.length = 0;
  $("far").dispatchEvent(new CustomEvent("ui:refresh", { bubbles: true }));
  await settle();
  assert.deepEqual(page.fetched.map((f) => f.url), []);
  assert.equal($("far").textContent, "kept");
});

test("a page that ended on another site loads its own address for real", async () => {
  page.answers.push(answer(`<!doctype html><html data-build="b1"><head><title>Evil</title></head><body data-ui-boost><main id="main">Evil</main></body></html>`, { url: "https://evil.example/landing", redirected: true }));
  page.assigned.length = 0;
  page.pushed.length = 0;
  $("next").click();
  await settle();
  assert.deepEqual(page.assigned, ["https://app.test/deals/2"]);
  assert.deepEqual(page.pushed, []);
  assert.equal($("main").textContent.includes("Evil"), false);
});

test("an upload to another site is never opened", async () => {
  const root = $("up");
  const input = root.querySelector("input");
  input.files = [{ name: "invoice.pdf", size: 10, type: "application/pdf" }];
  input.dispatchEvent(new Event("change", { bubbles: true }));
  await settle();
  assert.deepEqual(opened, []);
  assert.equal(root.querySelector("[data-slot=upload-item]")?.getAttribute("data-state"), "failed");
});
