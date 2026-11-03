// What the instant interactions must never do, in Chromium, Firefox and WebKit: send
// what waited to a page it wasn't typed on, write twice, write where a user types or
// into a field the page didn't open to commands, fill from the wrong row. The
// committed dist/ runs under the page's exact policy, served by tests/support/fixture.mjs.

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";
import { page as doc, start } from "./support/fixture.mjs";

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const until = async (done, ms = 5000) => {
  for (const end = Date.now() + ms; !done() && Date.now() < end; ) await sleep(20);
};
let unique = 0;
const key = (name) => `${name}-${++unique}`;
const button = (id, commands) => `<button id="${id}" type="button" data-ui-click='${JSON.stringify(commands).replace(/&/g, "&amp;").replace(/'/g, "&#39;")}'>x</button>`;

for (const [engine, launcher] of [["chromium", chromium], ["firefox", firefox], ["webkit", webkit]]) {
  describe(engine, () => {
    let browser, f;
    before(async () => {
      browser = await launcher.launch();
      f = await start();
    });
    after(async () => {
      await browser.close();
      f.close();
    });

    async function fresh(path) {
      const context = await browser.newContext();
      const page = await context.newPage();
      const errors = [];
      page.on("pageerror", (error) => errors.push(String(error)));
      await page.goto(f.origin + path, { waitUntil: "load" });
      await sleep(150);
      return { context, page, errors };
    }
    const route = (path, body, headers) => f.routes.set(path, (req, res) => f.html(res, typeof body === "function" ? body() : body, headers));
    const hits = (path) => f.log.filter((entry) => entry.path === path);
    const click = (page, id) => page.evaluate((b) => document.getElementById(b).click(), id);
    /** Submits "one", then "two" while the first is on its way. */
    const twice = (page) => page.evaluate(() => {
      const form = document.querySelector("form");
      form.requestSubmit(document.getElementById("go"));
      document.getElementById("i").value = "two";
      form.requestSubmit(document.getElementById("go"));
    });

    test(`text and fill never write into an option, a title or what a user types in (${engine})`, async () => {
      const name = key("closed");
      route(`/${name}`, doc(
        `<table><thead><tr><th data-col="title">T</th><th></th></tr></thead><tbody><tr data-key="7"><td>admin</td><td>${button("fillb", [["fill", "dlg"]])}</td></tr></tbody></table>` +
        `<form id="f"><select name="role" id="sel"><option id="o1">viewer</option></select></form>` +
        `<dialog id="dlg"><form id="g"><select name="role2"><option data-ui-cell="title">viewer</option></select></form><p id="ok" data-ui-cell="title">-</p></dialog>` +
        `<div id="ed" contenteditable="true"></div><div contenteditable="true"><span id="inner">inner</span></div>` +
        button("b1", [["text", "o1", "owner"], ["text", "ed", "replaced"], ["text", "inner", "replaced"], ["text", "ttl", "Sign in again"]])).replace("<title>t</title>", `<title id="ttl">t</title>`));
      const { context, page, errors } = await fresh(`/${name}`);
      await page.click("#ed");
      await page.keyboard.type("my draft");
      await click(page, "b1");
      await click(page, "fillb");
      const out = await page.evaluate(() => ({
        role: new FormData(document.getElementById("f")).get("role"),
        role2: new FormData(document.getElementById("g")).get("role2"),
        filled: document.getElementById("ok").textContent,
        ed: document.getElementById("ed").textContent,
        inner: document.getElementById("inner").textContent,
        title: document.title,
      }));
      assert.deepEqual(out, { role: "viewer", role2: "viewer", filled: "admin", ed: "my draft", inner: "inner", title: "t" });
      assert.deepEqual(errors, []);
      await context.close();
    });

    test(`value sets only a hidden field the page opened to commands (${engine})`, async () => {
      const name = key("value");
      route(`/${name}`, doc(
        `<div>${button("b1", [["value", "tok", "attacker"], ["value", "meth", "DELETE"], ["value", "plain", "x"], ["value", "sort", "amount"]])}</div>` +
        `<form id="other" method="post" action="/x"><input type="hidden" id="tok" name="_token" value="real"><input type="hidden" id="meth" name="_method" value="PUT"><input type="hidden" id="plain" name="plain" value="old"></form>` +
        `<form id="mine"><input type="hidden" id="sort" name="sort" value="title" data-ui-value></form>`));
      const { context, page } = await fresh(`/${name}`);
      await click(page, "b1");
      const out = await page.evaluate(() => ({
        other: Object.fromEntries(new FormData(document.getElementById("other"))),
        mine: Object.fromEntries(new FormData(document.getElementById("mine"))),
      }));
      assert.deepEqual(out, { other: { _token: "real", _method: "PUT", plain: "old" }, mine: { sort: "amount" } });
      await context.close();
    });
  });
}
