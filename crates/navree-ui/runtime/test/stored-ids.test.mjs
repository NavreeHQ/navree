// What a tab names by `aria-controls`, and what an upload names by `data-refresh`, is the element of the
// page with that id: one in stored content that has the same id, and stands ahead of it, is not it.
import { test } from "node:test";
import assert from "node:assert/strict";
import { install, settle } from "./fake-dom.mjs";

const page = install({
  url: "https://app.test/files",
  cookie: "XSRF-TOKEN=tok",
  html: `<!doctype html><html data-build="b1"><head></head><body>
    <div data-ui-stored><div id="second" data-slot="planted-panel">planted</div><div id="files" data-slot="planted-list">planted</div></div>
    <div id="tabs" data-ui-tabs="tab">
      <div role="tablist">
        <button id="first-tab" type="button" role="tab" aria-controls="first" aria-selected="true" data-key="a">First</button>
        <button id="second-tab" type="button" role="tab" aria-controls="second" aria-selected="false" data-key="b">Second</button>
      </div>
      <div id="first" role="tabpanel" aria-labelledby="first-tab">One</div>
      <div id="second" role="tabpanel" aria-labelledby="second-tab" hidden>Two</div>
    </div>
    <div id="files" data-slot="list">Files</div>
    <div id="up" data-ui-upload data-send="/files" data-refresh="files" data-l='{"done":"done","failed":"failed","offline":"offline"}'>
      <input type="file" name="file">
      <ul data-slot="upload-list"></ul><p data-slot="upload-status"></p>
      <template><li data-slot="upload-item"><span data-slot="attachment-title"></span><span data-slot="attachment-description"></span><progress></progress></li></template>
    </div>
  </body></html>`,
});

// A request that is answered at once, as a file that was stored.
Object.defineProperty(globalThis, "XMLHttpRequest", {
  configurable: true,
  writable: true,
  value: class {
    upload = {};
    status = 0;
    responseText = "";
    responseURL = "";
    open() {}
    setRequestHeader() {}
    abort() {}
    send() {
      setTimeout(() => {
        this.status = 200;
        this.responseText = `{"stored":[{"key":"k1"}]}`;
        this.responseURL = "https://app.test/files";
        /** @type {any} */ (this).onload();
      }, 0);
    }
  },
});

await import("../src/menus.js");
await import("../src/upload.js");
await settle();

/** The elements of the page with this id, in the order of the page. @param {string} id */
const twins = (id) => /** @type {any[]} */ ([...document.querySelectorAll(`[id=${id}]`)]);

test("a tab opens the panel the page drew, not an element in stored content with the same id", () => {
  const [planted, panel] = twins("second");
  assert.equal(planted.hasAttribute("data-slot"), true);
  /** @type {any} */ (document.getElementById("second-tab")).click();
  assert.equal(panel.hasAttribute("hidden"), false);
  assert.equal(panel.getAttribute("data-active"), "");
  assert.equal(planted.hasAttribute("data-active"), false);
  assert.equal(planted.hasAttribute("hidden"), false);
});

test("an upload that is over asks the region the page drew to draw itself again, not an element in stored content with the same id", async () => {
  const [planted, region] = twins("files");
  /** @type {any[]} */
  const asked = [];
  for (const el of [planted, region]) el.addEventListener("ui:refresh", () => asked.push(el));
  const input = /** @type {any} */ (document.querySelector("#up input"));
  input.files = [{ name: "invoice.pdf", size: 10, type: "application/pdf" }];
  input.dispatchEvent(new Event("change", { bubbles: true }));
  await settle();
  assert.deepEqual(asked, [region]);
});
