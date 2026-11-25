// The page that finishes an app's sign-in hands the sign-in on once, with a redirect to the app, which
// reaches the app only when the browser itself follows it. Two things keep the runtime out of its way,
// and neither is new code in the runtime: the sign-in pages tell it to leave for that address
// (`navigate_external`), which it does without a fetch; and the server changes nothing for a fetch that
// reaches the address all the same, and answers with a page that is none of the app's, which the runtime
// never shows: it sends the browser to the address the answer came from.
import { test } from "node:test";
import assert from "node:assert/strict";
import { answer, fire, install, settle } from "./fake-dom.mjs";

const FINISH = "/auth/app/finish?attempt=k3xq7aB2";
const THERE = "https://app.test" + FINISH;

const page = install({
  url: "https://app.test/sign-in?next=%2Fauth%2Fapp%2Ffinish%3Fattempt%3Dk3xq7aB2",
  cookie: "XSRF-TOKEN=tok",
  html: `<!doctype html><html data-build="b1"><head></head><body data-ui-boost>
    <main id="main">Sign in
      <form id="enter" method="post" action="/sign-in" data-ui-form>
        <input name="email" value="simran@example.in"><input type="hidden" name="next" value="${FINISH}">
        <button id="go" type="submit">Sign in</button>
      </form>
      <a id="link" href="${FINISH}">Continue to the app</a>
      <a id="ahead" href="${FINISH}" data-ui-prefetch>Continue to the app</a>
      <a id="done" href="/sign-in/done">Done</a>
    </main>
    <div id="kpis" data-ui-region data-ui-src="/fragments/kpis">₹1,00,000</div>
  </body></html>`,
});

const { refresh } = await import("../src/nav.js");
await import("../src/core.js");
await settle();

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
const OPS = { "content-type": "application/vnd.navree.ops+json" };
/**
 * What the server answers a fetch that reached the finish address: nothing changed, and a page that does
 * not say it is the app's. `sentOn`: the fetch asked for another address and was redirected there.
 * @param {boolean} sentOn
 */
const refusal = (sentOn) =>
  answer(`<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Opened by the browser only</title></head><body><p>This address is only opened by the browser itself.</p></body></html>`, {
    status: 403,
    url: THERE,
    redirected: sentOn,
    headers: { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" },
  });
const again = () => {
  page.fetched.length = 0;
  page.assigned.length = 0;
  page.pushed.length = 0;
  page.answers.length = 0;
};
const shown = () => $("main").textContent.includes("only opened by the browser");

test("told to leave for the finish address, the runtime sends the browser there and fetches nothing", async () => {
  again();
  page.answers.push(answer(JSON.stringify({ ops: [{ op: "navigate_external", href: FINISH }] }), { headers: OPS }));
  $("go").click();
  await settle();
  // One request: the form's own post.
  assert.deepEqual(
    page.fetched.map((f) => [f.init.method, f.url]),
    [["POST", "https://app.test/sign-in"]],
  );
  assert.deepEqual(page.assigned, [THERE]);
  assert.deepEqual(page.pushed, []);
});

test("a fetch of the finish address is not shown: the browser goes there", async () => {
  again();
  page.answers.push(refusal(false));
  $("link").click();
  await settle();
  assert.deepEqual(page.assigned, [THERE]);
  assert.deepEqual(page.pushed, []);
  assert.equal(shown(), false);
});

test("a page whose fetch was sent on to the finish address is not shown: the browser goes there", async () => {
  again();
  page.answers.push(refusal(true));
  $("done").click();
  await settle();
  // Not the link's own address: the address the answer came from.
  assert.deepEqual(page.assigned, [THERE]);
  assert.deepEqual(page.pushed, []);
  assert.equal(shown(), false);
});

test("a form's post that was sent on to the finish address sends the browser there, and says nothing failed", async () => {
  again();
  page.answers.push(refusal(true));
  $("go").click();
  await settle();
  assert.deepEqual(page.assigned, [THERE]);
  assert.equal($("enter").hasAttribute("data-pending"), false);
  assert.equal(shown(), false);
  assert.equal(document.body.textContent.includes("That didn't work"), false);
});

test("a region whose fetch was sent on to the finish address sends the browser there and keeps what it had", async () => {
  again();
  page.answers.push(refusal(true));
  await refresh("kpis");
  assert.deepEqual(page.assigned, [THERE]);
  assert.equal($("kpis").textContent, "₹1,00,000");
});

test("the finish address fetched ahead of a click never stands in for the click", async () => {
  again();
  page.answers.push(refusal(false), refusal(false));
  fire($("ahead"), "touchstart");
  await settle();
  assert.equal(page.fetched.length, 1);
  assert.deepEqual(page.assigned, []);
  $("ahead").click();
  await settle();
  // The answer fetched ahead is no page to show: the click asks again, and the browser goes there.
  assert.equal(page.fetched.length, 2);
  assert.deepEqual(page.assigned, [THERE]);
  assert.equal(shown(), false);
});
