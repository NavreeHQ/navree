// What the browser keeps between pages is the person's: a table's selection and a palette's recent
// items are kept for whoever is signed in, and a page kept for the back button that comes back to
// another person loads again.
import { test } from "node:test";
import assert from "node:assert/strict";
import { fire, install, settle } from "./fake-dom.mjs";

const page = install({
  url: "https://app.test/customers",
  cookie: "XSRF-TOKEN=harleen-token",
  html: `<!doctype html><html data-build="b1"><head></head><body data-ui-boost>
    <div id="t" data-ui-table data-l='{"selected":"{n} selected"}'>
      <div data-slot="table-bulk"><span data-slot="bulk-count"></span></div>
      <table><tbody><tr data-key="k1"><td><input id="box" type="checkbox" data-ui-select-row value="k1"></td></tr></tbody></table>
    </div>
    <dialog id="pal" data-ui-palette data-ui-recent="5" data-ui-picker>
      <input data-slot="command-input">
      <div data-slot="command-list" role="listbox">
        <div data-slot="command-recent" hidden></div>
        <div data-slot="command-item" role="option" data-value="gst" id="gst">GST summary</div>
      </div>
    </dialog>
  </body></html>`,
});

await import("../src/core.js");
await import("../src/table.js");
await import("../src/select.js");
await settle();

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
const signIn = (/** @type {string} */ token) => (document.cookie = `XSRF-TOKEN=${token}`);

test("a table's selection is the person's who made it", () => {
  signIn("harleen-token");
  $("box").checked = true;
  fire($("box"), "click");
  // The page is drawn again: the selection is read back.
  document.dispatchEvent(new Event("ui:swap"));
  assert.equal($("box").checked, true);
  // Another person signs in: the runtime sees it (here as the tab is shown again) before the page is drawn.
  signIn("gurpreet-token");
  document.dispatchEvent(new Event("visibilitychange"));
  document.dispatchEvent(new Event("ui:swap"));
  assert.equal($("box").checked, false);
  // No token in what is kept.
  for (let i = 0; i < sessionStorage.length; i++) assert.ok(!String(sessionStorage.key(i)).includes("token"));
});

test("a palette's recent items are the person's who chose them", () => {
  signIn("harleen-token");
  fire($("gst"), "click");
  fire($("pal"), "close");
  const recent = () => $("pal").querySelector("[data-slot=command-recent]").querySelectorAll("[data-slot=command-item]").length;
  assert.equal(recent(), 1);
  signIn("gurpreet-token");
  fire($("pal"), "close");
  assert.equal(recent(), 0);
});

test("a page kept for the back button that comes back to another person loads again", () => {
  signIn("harleen-token");
  dispatchEvent(new Event("pageshow", { persisted: true }));
  assert.equal(page.reloaded, 0);
  signIn("gurpreet-token");
  dispatchEvent(new Event("pageshow", { persisted: true }));
  assert.equal(page.reloaded, 1);
});
