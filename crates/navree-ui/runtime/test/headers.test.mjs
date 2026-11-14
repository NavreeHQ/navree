// What the runtime puts in a request header is made of bytes 0x21 to 0x7E: a browser refuses a value
// past Latin-1, so a region named in Gurmukhi must still refresh.
import { test } from "node:test";
import assert from "node:assert/strict";
import { answer, install, settle } from "./fake-dom.mjs";

const page = install({
  url: "https://app.test/ਗਾਹਕ?ਸ਼ਹਿਰ=ਲੁਧਿਆਣਾ",
  cookie: "XSRF-TOKEN=tok",
  html: `<!doctype html><html data-build="b1"><head></head><body>
    <div id="ਗਾਹਕ-ਸੂਚੀ" data-ui-region data-ui-src="/customers">Harleen Kaur</div>
    <div id="50%-off" data-ui-region data-ui-src="/offers">₹1,00,000</div>
  </body></html>`,
});

const { refresh } = await import("../src/nav.js");
await settle();

/** Whether a browser takes `value` as a header value. @param {string} value */
const headerSafe = (value) => /^[\x21-\x7e]*$/.test(value);

test("a region whose id is not in Latin letters asks for itself in a header a browser sends", async () => {
  page.fetched.length = 0;
  page.answers.push(answer(`<div id="ਗਾਹਕ-ਸੂਚੀ">Gurpreet Singh</div>`, { headers: { "content-type": "text/html", "x-navree-fragment": "1" } }));
  await refresh("ਗਾਹਕ-ਸੂਚੀ");
  const headers = page.fetched[0].init.headers;
  for (const [name, value] of Object.entries(headers)) assert.ok(headerSafe(String(value)), `${name}: ${value}`);
  assert.equal(decodeURIComponent(headers["x-navree-region"]), "ਗਾਹਕ-ਸੂਚੀ");
});

test("a `%` in a region's id is encoded too", async () => {
  page.fetched.length = 0;
  page.answers.push(answer(`<div id="50%-off">₹50,000</div>`));
  await refresh("50%-off");
  assert.equal(page.fetched[0].init.headers["x-navree-region"], "50%25-off");
});
