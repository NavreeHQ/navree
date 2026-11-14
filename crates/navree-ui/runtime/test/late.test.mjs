// Late answers: what was fetched before a write, a sign-out or another tab's change is fetched again,
// not shown; a navigation that began before a form's post never replaces the form's answer; a region's
// fetch from one page never lands in the next page's region of the same id; and a page dropped as
// overtaken leaves no history entry.
import { test } from "node:test";
import assert from "node:assert/strict";
import { answer, install, settle } from "./fake-dom.mjs";

const page = install({
  url: "https://app.test/deals",
  cookie: "XSRF-TOKEN=tok",
  html: `<!doctype html><html data-build="b1"><head></head><body data-ui-boost>
    <main id="main">Deals
      <a id="one" href="/deals/1">1</a><a id="two" href="/deals/2">2</a><a id="three" href="/deals/3">3</a>
      <form id="note" method="post" action="/deals/notes" data-ui-form><button id="save" type="submit">Save</button></form>
    </main>
    <div id="kpis" data-ui-region data-ui-src="/deals/kpis">₹1,00,000</div>
  </body></html>`,
});

const { refresh } = await import("../src/nav.js");
await import("../src/core.js");
await settle();

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
const PAGE = { "content-type": "text/html; charset=utf-8", "x-navree-page": "1" };
const OPS = { "content-type": "application/vnd.navree.ops+json" };
/** A page of the app with `main` in it. @param {string} main @param {string} [kpis] @param {string} [headExtra] */
const html = (main, kpis = "₹1,00,000", headExtra = "") =>
  `<!doctype html><html data-build="b1"><head><title>${main}</title>${headExtra}</head><body data-ui-boost><main id="main">${main}</main><div id="kpis" data-ui-region data-ui-src="/deals/kpis">${kpis}</div></body></html>`;
/** An answer the test lets go of when it wants. */
function held() {
  /** @type {(value: any) => void} */
  let release = () => {};
  const promise = new Promise((done) => (release = done));
  return { promise, release };
}

test("a page fetched before another tab's change is fetched again, not shown", async () => {
  page.fetched.length = 0;
  page.pushed.length = 0;
  page.answers.push(
    () => {
      // Another tab wrote while this answer was on its way.
      page.channels[0].onmessage();
      return answer(html("Deal 1, as it was"), { headers: PAGE });
    },
    answer(html("Deal 1"), { headers: PAGE }),
  );
  $("one").click();
  await settle();
  assert.equal(page.fetched.length, 2);
  assert.equal($("main").textContent, "Deal 1");
  assert.deepEqual(page.pushed, ["https://app.test/deals/1"]);
});

test("a navigation that began before a form's post never replaces the form's answer", async () => {
  history.replaceState(null, "", "/deals");
  $("main").innerHTML = `Deals <a id="two" href="/deals/2">2</a><form id="note" method="post" action="/deals/notes" data-ui-form><button id="save" type="submit">Save</button></form>`;
  const slow = held();
  page.answers.push(() => slow.promise, answer(`{"ops":[{"op":"toast","tone":"success","text":"Saved"}]}`, { headers: OPS }));
  page.pushed.length = 0;
  $("two").click();
  await settle(2);
  $("save").click();
  await settle();
  slow.release(answer(html("Deal 2"), { headers: PAGE }));
  await settle();
  assert.equal($("main").textContent.includes("Deal 2"), false);
  assert.deepEqual(page.pushed, []);
  assert.deepEqual(page.assigned, []);
});

test("a region's fetch from one page doesn't land in the next page's region of the same id", async () => {
  $("main").innerHTML = `<a id="three" href="/deals/3">3</a>`;
  const slow = held();
  page.answers.push(() => slow.promise);
  const fetching = refresh("kpis");
  await settle(2);
  page.answers.push(answer(html("Deal 3", "₹3,00,000"), { headers: PAGE }));
  $("three").click();
  await settle();
  assert.equal($("kpis").textContent, "₹3,00,000");
  slow.release(answer(`<div id="kpis" data-ui-region data-ui-src="/deals/kpis">₹9,99,999</div>`, { headers: PAGE }));
  await fetching;
  await settle();
  assert.equal($("kpis").textContent, "₹3,00,000");
});

test("a page dropped as overtaken while its styles load leaves no history entry", { timeout: 10000 }, async () => {
  history.replaceState(null, "", "/deals/3");
  $("main").innerHTML = `<a id="four" href="/deals/4">4</a><a id="five" href="/deals/5">5</a>`;
  page.pushed.length = 0;
  page.answers.push(answer(html("Deal 4", "₹4,00,000", `<link rel="stylesheet" href="/_navree/ui/theme-4.css">`), { headers: PAGE }));
  $("four").click();
  await settle();
  page.answers.push(answer(html("Deal 5", "₹5,00,000"), { headers: PAGE }));
  $("five").click();
  await settle();
  // The first waits for its stylesheet (four seconds at most), then finds itself overtaken.
  await new Promise((done) => setTimeout(done, 4500));
  assert.deepEqual(page.pushed, ["https://app.test/deals/5"]);
  assert.equal($("main").textContent, "Deal 5");
});

// Something written on every answer would have a navigation, or a region, fetch for ever. It asks again
// twice at most, then shows what came.
test("a page whose every answer is overtaken by a write is shown after at most three fetches", async () => {
  history.replaceState(null, "", "/deals/5");
  $("main").innerHTML = `<a id="six" href="/deals/6">6</a>`;
  page.fetched.length = 0;
  page.pushed.length = 0;
  page.assigned.length = 0;
  for (let n = 1; n <= 6; n++) {
    page.answers.push(() => {
      // Another tab wrote while this answer was on its way.
      page.channels[0].onmessage();
      return answer(html(`Deal 6, answer ${n}`), { headers: PAGE });
    });
  }
  $("six").click();
  await settle();
  assert.equal(page.fetched.length, 3);
  assert.equal($("main").textContent, "Deal 6, answer 3");
  assert.deepEqual(page.pushed, ["https://app.test/deals/6"]);
  assert.deepEqual(page.assigned, []);
  page.answers.length = 0;
});

test("a region whose every answer is overtaken by a write is drawn after at most three fetches", async () => {
  page.fetched.length = 0;
  for (let n = 1; n <= 6; n++) {
    page.answers.push(() => {
      page.channels[0].onmessage();
      return answer(`<div id="kpis" data-ui-region data-ui-src="/deals/kpis">₹${n},00,000</div>`, { headers: PAGE });
    });
  }
  await refresh("kpis");
  await settle();
  assert.equal(page.fetched.length, 3);
  assert.equal($("kpis").textContent, "₹3,00,000");
  page.answers.length = 0;
});
