// A composer form and an upload area act only on controls and drops of their own: a textarea, file input
// or drop target inside stored content or inside a sealed body neither submits the composer nor sends files
// to the upload area around it.
import { test } from "node:test";
import assert from "node:assert/strict";
import { fire, install, settle } from "./fake-dom.mjs";

const page = install({
  url: "https://app.test/assistant",
  cookie: "XSRF-TOKEN=tok",
  html: `<!doctype html><html data-build="b1"><head><template id="ui-sealed"></template></head><body>
    <form id="composer" method="post" action="/assistant/ask" data-genui="composer">
      <div id="composer-sealed" data-ui-keep data-ui-stored>
        <textarea id="sealed-area" name="draft" form="ui-sealed">Send from Ludhiana</textarea>
      </div>
      <div id="composer-stored" data-ui-stored>
        <textarea id="stored-area" name="quoted">Quoted from Amritsar</textarea>
      </div>
      <textarea id="detached-area" name="elsewhere" form="other-form">Detached note</textarea>
      <textarea id="own-area" name="prompt">Check Jalandhar depot</textarea>
    </form>
    <form id="other-form" method="post" action="/other"></form>

    <div id="upload" data-ui-upload data-send="/assistant/files" data-l='{"done":"done","failed":"failed","offline":"offline"}'>
      <div id="upload-sealed" data-ui-keep data-ui-stored>
        <input id="sealed-file" type="file" name="file" form="ui-sealed">
        <div id="sealed-drop">Drop inside pad</div>
      </div>
      <input id="own-file" type="file" name="file">
      <div id="own-drop">Drop invoice here</div>
      <ul data-slot="upload-list"></ul>
      <p data-slot="upload-status"></p>
      <template><li data-slot="upload-item"><span data-slot="attachment-title"></span><span data-slot="attachment-description"></span><progress></progress></li></template>
    </div>
  </body></html>`,
});

/** @type {string[]} */
const uploaded = [];
Object.defineProperty(globalThis, "XMLHttpRequest", {
  configurable: true,
  writable: true,
  value: class {
    upload = {};
    status = 0;
    responseText = "";
    responseURL = "";
    /** @param {string} _method @param {string} url */
    open(_method, url) {
      this._url = url;
    }
    setRequestHeader() {}
    abort() {}
    /** @param {FormData} body */
    send(body) {
      const sent = /** @type {any} */ ([...body.entries()].find(([k]) => k === "file")?.[1]);
      uploaded.push(`${this._url}:${sent?.name || ""}`);
      setTimeout(() => {
        this.status = 200;
        this.responseText = `{"stored":[{"key":"k1"}]}`;
        this.responseURL = "https://app.test/assistant/files";
        /** @type {any} */ (this).onload();
      }, 0);
    }
  },
});

await import("../src/genui.js");
await import("../src/upload.js");
await settle();

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));

test("Enter in a textarea inside stored or sealed markup does not send the composer around it", () => {
  page.submitted.length = 0;
  for (const id of ["sealed-area", "stored-area", "detached-area"]) {
    const ev = fire($(id), "keydown", { key: "Enter" });
    assert.equal(ev.defaultPrevented, false, id);
  }
  assert.deepEqual(page.submitted, []);

  const own = fire($("own-area"), "keydown", { key: "Enter" });
  assert.equal(own.defaultPrevented, true);
  assert.equal(page.submitted.length, 1);
  assert.equal(page.submitted[0].form, $("composer"));
});

test("choosing or dropping a file inside stored or sealed markup does not upload to the area around it", async () => {
  uploaded.length = 0;
  const receipt = new File(["x".repeat(512)], "receipt.pdf", { type: "application/pdf" });

  $("sealed-file").files = [receipt];
  fire($("sealed-file"), "change");

  const dataTransfer = { types: ["Files"], files: [receipt], dropEffect: "copy" };
  fire($("sealed-drop"), "dragenter", { dataTransfer });
  assert.equal($("upload").hasAttribute("data-drag"), false);
  const droppedInSealed = fire($("sealed-drop"), "drop", { dataTransfer });
  assert.equal(droppedInSealed.defaultPrevented, true);
  await settle();
  assert.deepEqual(uploaded, []);

  // The area's own input and drop surface still send as before.
  $("own-file").files = [receipt];
  fire($("own-file"), "change");
  await settle();
  assert.deepEqual(uploaded, ["/assistant/files:receipt.pdf"]);

  const challan = new File(["x".repeat(640)], "challan.pdf", { type: "application/pdf" });
  const ownTransfer = { types: ["Files"], files: [challan], dropEffect: "copy" };
  fire($("own-drop"), "drop", { dataTransfer: ownTransfer });
  await settle();
  assert.deepEqual(uploaded, ["/assistant/files:receipt.pdf", "/assistant/files:challan.pdf"]);
});
