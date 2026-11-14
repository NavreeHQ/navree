// Which addresses the islands module imports: a script of the app's own under the mount, named by
// its content hash, with a plugin's namespace and a dot before the name when it is a plugin's.
// A script cannot load here, so an address that is tried marks its island `data-ui-failed`, and
// an address that is refused leaves its island untouched.
import { test } from "node:test";
import assert from "node:assert/strict";
import { install, settle } from "./fake-dom.mjs";

/** @type {Record<string, string>} */
const tried = {
  plain: "/_navree/ui/island/clock-0123456789ab.js",
  namespaced: "/_navree/ui/island/announcements.clock-0123456789ab.js",
  "namespace-with-underscore": "/_navree/ui/island/notice_board.depot-map-0123456789ab.js",
};

/** @type {Record<string, string>} */
const refused = {
  "two-namespaces": "/_navree/ui/island/announcements.extra.clock-0123456789ab.js",
  "upper-case-namespace": "/_navree/ui/island/Announcements.clock-0123456789ab.js",
  "namespace-starting-with-a-digit": "/_navree/ui/island/9lives.clock-0123456789ab.js",
  "empty-namespace": "/_navree/ui/island/.clock-0123456789ab.js",
  "dot-in-the-name": "/_navree/ui/island/announcements.clock.old-0123456789ab.js",
  "no-hash": "/_navree/ui/island/announcements.clock.js",
  "a-folder-above": "/_navree/ui/island/../core.js",
  "another-origin": "https://other.test/_navree/ui/island/announcements.clock-0123456789ab.js",
  "with-a-query": "/_navree/ui/island/announcements.clock-0123456789ab.js?v=2",
};

const islands = Object.entries({ ...tried, ...refused })
  .map(([id, src]) => `<div id="${id}" data-ui-island="${src}" data-ui-props="{}"></div>`)
  .join("\n");

install({
  url: "https://app.test/notices",
  html: `<!doctype html><html data-build="b1"><head></head><body>${islands}</body></html>`,
});

await import("../src/islands.js");
await settle();

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));

for (const id of Object.keys(tried)) {
  test(`an island script address is imported: ${id}`, () => {
    assert.equal($(id).hasAttribute("data-ui-failed"), true);
  });
}

for (const id of Object.keys(refused)) {
  test(`an island script address is refused: ${id}`, () => {
    assert.equal($(id).hasAttribute("data-ui-failed"), false);
    assert.equal($(id).hasAttribute("data-ui-mounted"), false);
  });
}
