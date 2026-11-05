// What the instant interactions must never do, in Chromium, Firefox and WebKit: send
// what waited to a page it wasn't typed on, write twice, write where a user types or
// into a field the page didn't open to commands, fill from the wrong row. The
// committed dist/ runs under the page's exact policy, served by tests/support/fixture.mjs.

import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";
import { page as doc, start } from "./support/fixture.mjs";
import { launch } from "./support/launch.mjs";

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const until = async (done, ms = 5000) => {
  for (const end = Date.now() + ms; !done() && Date.now() < end; ) await sleep(20);
};
let unique = 0;
const key = (name) => `${name}-${++unique}`;
// A cold browser on a busy machine can take seconds to swap a page: waits for a state are
// long, and the checks that nothing else was sent come after the state is reached.
const LONG = 30000;
const button = (id, commands) => `<button id="${id}" type="button" data-ui-click='${JSON.stringify(commands).replace(/&/g, "&amp;").replace(/'/g, "&#39;")}'>x</button>`;

for (const [engine, launcher] of [["chromium", chromium], ["firefox", firefox], ["webkit", webkit]]) {
  describe(engine, () => {
    let browser, f;
    before(async () => {
      browser = await launch(launcher);
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

    for (const withId of [true, false]) {
      const id = withId ? ` id="f"` : "";
      const label = withId ? "with an id" : "with none";

      test(`what waited is dropped when the answer is a redirect to another page, form ${label} (${engine})`, async () => {
        const name = key("redirect");
        route(`/${name}`, doc(`<main><form${id} data-ui-form method="post" action="/${name}-post"><input id="i" name="title" value="one"><button id="go">Save</button></form></main>`));
        f.routes.set(`/${name}-post`, (req, res) => setTimeout(() => { res.writeHead(303, { location: `/${name}-next` }); res.end(); }, 300));
        route(`/${name}-next`, doc(`<main><form${id} data-ui-form method="post" action="/${name}-danger"><input type="hidden" name="all" value="yes"><button id="go">Delete every deal</button></form></main>`));
        f.routes.set(`/${name}-danger`, (req, res) => f.ops(res, { ops: [] }));
        const { context, page } = await fresh(`/${name}`);
        await twice(page);
        await until(() => new URL(page.url()).pathname === `/${name}-next`, LONG);
        await sleep(800);
        assert.equal(new URL(page.url()).pathname, `/${name}-next`);
        assert.deepEqual(hits(`/${name}-danger`), [], "nothing posted on the page the user was sent to");
        assert.equal(hits(`/${name}-post`).length, 1, "and what waited isn't sent anywhere");
        await context.close();
      });

      test(`what waited is dropped when the user goes to another page meanwhile, form ${label} (${engine})`, async () => {
        const name = key("navaway");
        route(`/${name}`, doc(`<nav><a id="away" href="/${name}-other">Other</a></nav><main><form${id} data-ui-form method="post" action="/${name}-post"><input id="i" name="title" value="one"><button id="go">Save</button></form></main>`));
        f.routes.set(`/${name}-post`, (req, res) => setTimeout(() => f.ops(res, { ops: [{ op: "toast", tone: "success", text: "saved" }] }), 500));
        route(`/${name}-other`, doc(`<nav><a href="/${name}">Back</a></nav><main><form${id} data-ui-form method="post" action="/${name}-danger"><input type="hidden" name="archive" value="all"><button id="go">Archive all</button></form></main>`));
        f.routes.set(`/${name}-danger`, (req, res) => f.ops(res, { ops: [] }));
        const { context, page } = await fresh(`/${name}`);
        await twice(page);
        await page.click("#away");
        await until(() => new URL(page.url()).pathname === `/${name}-other`, LONG);
        await sleep(1200);
        assert.equal(new URL(page.url()).pathname, `/${name}-other`);
        assert.deepEqual(hits(`/${name}-danger`), [], "nothing posted on the other page");
        assert.equal(hits(`/${name}-post`).length, 1);
        await context.close();
      });
    }

    test(`what waited goes after an ops answer on the same page, and only there (${engine})`, async () => {
      const name = key("same");
      route(`/${name}`, doc(`<form id="f" data-ui-form method="post" action="/${name}-post"><input id="i" name="title" value="one"><button id="go">Save</button></form>`));
      f.routes.set(`/${name}-post`, (req, res, body) => setTimeout(() => f.ops(res, { ops: [] }), 250));
      const { context, page } = await fresh(`/${name}`);
      await twice(page);
      await until(() => hits(`/${name}-post`).length >= 2, LONG);
      await sleep(300);
      assert.deepEqual(hits(`/${name}-post`).map((hit) => hit.body), ["title=one", "title=two"]);
      await context.close();
    });

    test(`what waited is dropped when the form now posts somewhere else (${engine})`, async () => {
      const name = key("moved");
      route(`/${name}`, doc(`<form id="f" data-ui-form method="post" action="/${name}-post"><input id="i" name="title" value="one"><button id="go">Save</button></form>`));
      f.routes.set(`/${name}-post`, (req, res) => setTimeout(() => f.ops(res, { ops: [{ op: "morph", target: "f", html: `<form id="f" data-ui-form method="post" action="/${name}-danger"><input id="i" name="title" value="two"><button id="go">Delete</button></form>` }] }), 250));
      f.routes.set(`/${name}-danger`, (req, res) => f.ops(res, { ops: [] }));
      const { context, page } = await fresh(`/${name}`);
      await twice(page);
      await page.waitForFunction((n) => document.getElementById("f")?.getAttribute("action") === `/${n}-danger`, name, { timeout: LONG });
      await sleep(900);
      assert.deepEqual(hits(`/${name}-danger`), []);
      await context.close();
    });

    test(`what waited is dropped when the answer signed the user out (${engine})`, async () => {
      const name = key("token");
      route(`/${name}`, doc(`<form id="f" data-ui-form method="post" action="/${name}-post"><input id="i" name="a" value="one"><button id="go">go</button></form>`), { "set-cookie": `XSRF-TOKEN=tok1; Path=/` });
      f.routes.set(`/${name}-post`, (req, res) => setTimeout(() => f.ops(res, { ops: [] }, 200, { "set-cookie": "XSRF-TOKEN=; Path=/; Max-Age=0" }), 300));
      const { context, page } = await fresh(`/${name}`);
      await twice(page);
      await page.waitForFunction(() => !document.cookie.includes("XSRF-TOKEN=tok1"), null, { timeout: LONG });
      await sleep(1000);
      assert.deepEqual(hits(`/${name}-post`).map((hit) => [hit.body, hit.headers["x-xsrf-token"] ?? null]), [["a=one", "tok1"]]);
      await context.close();
    });

    test(`a double click writes once on a form with an empty optional file field (${engine})`, async () => {
      for (const how of ["double click", "requestSubmit twice"]) {
        const name = key("file");
        let writes = 0;
        let release;
        const held = new Promise((resolve) => (release = resolve));
        route(`/${name}`, doc(`<form id="f" data-ui-form method="post" enctype="multipart/form-data" action="/${name}-post"><input name="title" value="Acme"><input type="file" name="attachment"><button id="go">Create</button></form>`));
        f.routes.set(`/${name}-post`, async (req, res) => {
          writes++;
          await held;
          f.ops(res, { ops: [{ op: "toast", tone: "info", text: "created" }] });
        });
        const { context, page } = await fresh(`/${name}`);
        if (how === "double click") await page.dblclick("#go");
        else await page.evaluate(async () => {
          const form = document.getElementById("f");
          form.requestSubmit();
          await new Promise((resolve) => setTimeout(resolve, 5));
          form.requestSubmit();
        });
        await sleep(300);
        release();
        await sleep(600);
        assert.equal(writes, 1, how);
        await context.close();
      }
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

    test(`fill from a button in a details row fills from the row it details (${engine})`, async () => {
      const name = key("detail");
      route(`/${name}`, doc(
        `<table><thead><tr><th data-col="title">Title</th><th data-col="amount">Amount</th></tr></thead><tbody>` +
        `<tr data-key="7"><td>Acme</td><td>70</td></tr><tr data-slot="row-detail" data-key="7-detail"><td colspan="2">details ${button("inner", [["fill", "dlg"]])}</td></tr>` +
        `</tbody></table><dialog id="dlg"><b id="t" data-ui-cell="title">-</b><i id="a" data-ui-cell="amount">-</i><form><input type="hidden" id="k" name="id" data-ui-row-key></form></dialog>`));
      const { context, page } = await fresh(`/${name}`);
      await click(page, "inner");
      const out = await page.evaluate(() => ({ title: document.getElementById("t").textContent, amount: document.getElementById("a").textContent, key: document.getElementById("k").value }));
      assert.deepEqual(out, { title: "Acme", amount: "70", key: "7" });
      await context.close();
    });

    /** An inline-edit list: each row a small form posting to one address, the record's id hidden. Saving moves the row to the top. */
    const notes = (name, items, keyed) => `<ul id="list">${items.map(([id, note]) => `<li${keyed ? ` data-key="${id}"` : ""}><form data-ui-form method="post" action="/${name}-save"><input type="hidden" name="id" value="${id}"><input name="note" value="${note}"><button>Save</button></form></li>`).join("")}</ul>`;
    const saveMovesToTop = (name, items, keyed) => (req, res, body) => {
      const sent = new URLSearchParams(body);
      const [row] = items.splice(items.findIndex(([id]) => id === sent.get("id")), 1);
      row[1] = sent.get("note");
      items.unshift(row);
      setTimeout(() => f.ops(res, { ops: [{ op: "morph", target: "list", html: notes(name, items, keyed) }] }), 300);
    };
    /** Saves the third row's note, then types more and saves again while the first is on its way. */
    const editThird = (page) => page.evaluate(() => {
      const form = document.querySelectorAll("form")[2];
      form.querySelector("[name=note]").value = "charlie 2";
      form.requestSubmit();
      form.querySelector("[name=note]").value = "charlie 3";
      form.requestSubmit();
    });

    test(`what waited is dropped, and said so, when the answer put another record in its form (${engine})`, async () => {
      const name = key("record");
      const items = [["A", "alpha"], ["B", "bravo"], ["C", "charlie"]];
      route(`/${name}`, () => doc(notes(name, items, false)));
      f.routes.set(`/${name}-save`, saveMovesToTop(name, items, false));
      const { context, page } = await fresh(`/${name}`);
      await editThird(page);
      // The list has no keys: after the answer, the third form is B's.
      await page.locator("[data-slot=toast][data-tone=warning]").waitFor({ timeout: LONG });
      await sleep(600);
      assert.deepEqual(hits(`/${name}-save`).map((hit) => hit.body), ["id=C&note=charlie+2"], "nothing written to B");
      await context.close();
    });

    test(`what waited goes to its own record when the list is keyed and moves it (${engine})`, async () => {
      const name = key("keyed");
      const items = [["A", "alpha"], ["B", "bravo"], ["C", "charlie"]];
      route(`/${name}`, () => doc(notes(name, items, true)));
      f.routes.set(`/${name}-save`, saveMovesToTop(name, items, true));
      const { context, page } = await fresh(`/${name}`);
      await editThird(page);
      await until(() => hits(`/${name}-save`).length >= 2, LONG);
      await sleep(600);
      assert.deepEqual(hits(`/${name}-save`).map((hit) => hit.body), ["id=C&note=charlie+2", "id=C&note=charlie+3"]);
      assert.equal(await page.locator("[data-slot=toast][data-tone=warning]").count(), 0);
      await context.close();
    });

    test(`what waited is dropped, and said so, when the answer is this page again; the typed text stays (${engine})`, async () => {
      const name = key("again");
      let saved = "one";
      f.routes.set(`/${name}`, (req, res, body) => {
        const answer = () => f.html(res, doc(`<form data-ui-form method="post" action="/${name}"><input id="i" name="title" value="${saved}"><button id="go">Save</button></form>`));
        if (req.method !== "POST") return answer();
        saved = new URLSearchParams(body).get("title");
        setTimeout(answer, 300);
      });
      const { context, page } = await fresh(`/${name}`);
      await twice(page);
      await page.locator("[data-slot=toast][data-tone=warning]").waitFor({ timeout: LONG });
      await sleep(600);
      assert.deepEqual(hits(`/${name}`).filter((hit) => hit.method === "POST").map((hit) => hit.body), ["title=one"]);
      assert.equal(await page.inputValue("#i"), "two", "what the user typed is still there to save");
      await context.close();
    });

    test(`a file picked again, with the same name, size and type, is a new submit (${engine})`, async () => {
      const name = key("repick");
      let writes = 0;
      route(`/${name}`, doc(`<form id="f" data-ui-form method="post" enctype="multipart/form-data" action="/${name}-post"><input type="file" id="file" name="file"><button id="go">Upload</button></form>`));
      f.routes.set(`/${name}-post`, (req, res) => {
        writes++;
        setTimeout(() => f.ops(res, { ops: [] }), 300);
      });
      const { context, page } = await fresh(`/${name}`);
      await page.evaluate(() => {
        const pick = (stamp) => {
          const files = new DataTransfer();
          files.items.add(new File(["same"], "report.csv", { type: "text/csv", lastModified: stamp }));
          document.getElementById("file").files = files.files;
        };
        const form = document.getElementById("f");
        pick(1000);
        form.requestSubmit(document.getElementById("go"));
        pick(2000);
        form.requestSubmit(document.getElementById("go"));
      });
      await until(() => writes >= 2, LONG);
      await sleep(300);
      assert.equal(writes, 2);
      await context.close();
    });
  });
}
