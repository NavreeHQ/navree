// A named element shadows a property of the document or of its form (`<img name="addEventListener">`,
// `<input name="requestSubmit">`). The table module and the forms read through the prototypes, so
// stored markup with such a name changes nothing they do.
import { test } from "node:test";
import assert from "node:assert/strict";
import { answer, fire, install, settle } from "./fake-dom.mjs";

const page = install({
  url: "https://app.test/invoices",
  cookie: "XSRF-TOKEN=tok",
  html: `<!doctype html><html data-build="b1"><head></head><body data-ui-boost>
    <img name="addEventListener"><img name="querySelectorAll"><img name="getElementById"><img name="createElement">
    <div id="t" data-ui-table data-total="2" data-l='{"selected":"{n} selected"}'>
      <div data-slot="table-bulk"><span data-slot="bulk-count"></span></div>
      <form id="bulk" method="post" action="/invoices/bulk" data-ui-bulk>
        <input name="requestSubmit">
        <button id="void" type="submit" data-ui-confirm="Cancel {n} invoices?">Cancel</button>
      </form>
      <table><tbody><tr data-key="k1"><td><input id="box" type="checkbox" data-ui-select-row value="k1"></td></tr></tbody></table>
      <form id="edit" method="post" action="/invoices/k1/title" data-ui-edit><input id="cell" name="title" value="Paid"><input name="requestSubmit"></form>
      <dialog id="t-confirm"><p data-slot="dialog-description"></p><button id="yes" type="button" data-ui-confirm-yes>Yes</button></dialog>
    </div>
    <form id="plain" method="get" action="/invoices/search"><input name="q" value="UPI"><input name="requestSubmit"></form>
    <button id="send-plain" type="button" data-ui-click='[["submit","plain"]]'>Search</button>
    <form id="note" method="post" action="/invoices/1/note" data-ui-form>
      <input id="text" name="text" value="Paid by UPI"><input name="requestSubmit">
      <button id="save" type="submit">Save</button>
    </form>
  </body></html>`,
});

// What a browser does with those names: the document's properties, and the forms', are the elements.
for (const name of ["addEventListener", "querySelectorAll", "getElementById", "createElement"]) {
  Object.defineProperty(document, name, { value: document.querySelector(`img[name=${name}]`), configurable: true });
}
const $ = (/** @type {string} */ id) => /** @type {any} */ (Document.prototype.getElementById.call(document, id));
for (const [form, name] of [["note", "requestSubmit"]]) {
  Object.defineProperty($(form), name, { value: $(form).querySelector(`[name=${name}]`), configurable: true });
}

await import("../src/core.js");
await import("../src/table.js");
await settle();

test("the table module starts on a page with an element named like a document method", () => {
  $("box").checked = true;
  fire($("box"), "click");
  assert.equal($("t").hasAttribute("data-ui-ready"), true);
  assert.equal($("t").hasAttribute("data-has-selection"), true);
});

test("a bulk form asks first on a page with an element named like a document method", async () => {
  page.submitted.length = 0;
  page.fetched.length = 0;
  $("bulk").requestSubmit($("void"));
  await settle();
  assert.equal($("t-confirm").hasAttribute("open"), true);
  assert.deepEqual(page.submitted, []);
  assert.deepEqual(page.fetched, []);
});

test("what was typed while a save was on its way is sent after it, whatever the form's fields are named", async () => {
  /** @type {(value: any) => void} */
  let release = () => {};
  page.fetched.length = 0;
  page.answers.push(
    () => new Promise((done) => (release = done)),
    answer(`{"ops":[]}`, { headers: { "content-type": "application/vnd.navree.ops+json" } }),
  );
  $("save").click();
  await settle(2);
  $("text").value = "Paid by UPI on 14 November";
  $("save").click();
  release(answer(`{"ops":[]}`, { headers: { "content-type": "application/vnd.navree.ops+json" } }));
  await settle();
  assert.equal(page.fetched.length, 2);
  assert.match(String(page.fetched[1].init.body), /14\+November/);
});

/** What a browser does with a field named like a form's method: the form's property is the field. @param {string} form */
function shadowed(form) {
  Object.defineProperty($(form), "requestSubmit", { value: $(form).querySelector("[name=requestSubmit]"), configurable: true });
  return () => delete $(form).requestSubmit;
}

test("the submit command sends a form that has a field named like the method", async () => {
  page.submitted.length = 0;
  page.errors.length = 0;
  const restore = shadowed("plain");
  $("send-plain").click();
  await settle();
  restore();
  assert.deepEqual(page.errors, []);
  assert.equal(page.submitted.length, 1);
  assert.equal(page.submitted[0].form, $("plain"));
});

test("a bulk form that asked first sends itself after the answer, whatever its fields are named", async () => {
  page.submitted.length = 0;
  page.errors.length = 0;
  sessionStorage.setItem("navree-table:t:/invoices", JSON.stringify({ mode: "ids", ids: ["k1"], ex: [], key: "" }));
  const restore = shadowed("bulk");
  $("void").click();
  assert.equal($("t-confirm").hasAttribute("open"), true);
  assert.deepEqual(page.submitted, []);
  $("yes").click();
  await settle();
  restore();
  assert.deepEqual(page.errors, []);
  assert.equal(page.submitted.length, 1);
  assert.equal(page.submitted[0].form, $("bulk"));
  assert.equal(page.submitted[0].submitter, $("void"));
});

test("an edited cell sends its form whatever the form's fields are named", async () => {
  page.submitted.length = 0;
  page.errors.length = 0;
  const restore = shadowed("edit");
  $("cell").value = "Paid by NEFT";
  fire($("cell"), "change");
  await settle();
  restore();
  assert.deepEqual(page.errors, []);
  assert.equal(page.submitted.length, 1);
  assert.equal(page.submitted[0].form, $("edit"));
});
