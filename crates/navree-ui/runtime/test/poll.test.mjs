// A polled region is fetched once a second at the most, whatever its `data-ui-poll` says.
import { test } from "node:test";
import assert from "node:assert/strict";
import { install } from "./fake-dom.mjs";

const page = install({
  url: "https://app.test/",
  html: `<!doctype html><html data-build="b1"><head></head><body>
    <div id="kpis" data-ui-region data-ui-src="/fragments/kpis" data-ui-poll="5"></div>
  </body></html>`,
});

await import("../src/core.js");

test("a poll asked for every few milliseconds waits a second", async () => {
  await new Promise((done) => setTimeout(done, 400));
  assert.equal(page.fetched.length, 0);
});
