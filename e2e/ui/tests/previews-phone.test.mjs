// The docs' previews of the theme menu, the navigation menu and the date and time boxes, in the frame a
// phone gives them. This runs the very documents the website serves, from `docs/ui-data/previews`, with
// the toolkit's committed stylesheets and modules, so it needs no other server. On a phone 390 px wide
// the docs draw a preview in a frame 356 px wide (341 px on a 375 px phone, 326 px on a 360 px one), and
// a preview keeps 24 px of padding on each side of what it shows. In that room:
//
// - a menu or a panel drawn open in place is no wider than what holds it as it loads, keeps a margin
//   from the frame's edge when its own button opens it over the page, and is back in its place once closed;
// - a date or a time box in a gallery shows the whole of what it holds beside its buttons;
// - the frame never scrolls sideways.
//
// PREVIEW_BROWSERS="chromium firefox webkit" picks browsers.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, join } from "node:path";
import { after, before, describe, test } from "node:test";
import { fileURLToPath } from "node:url";
import * as playwright from "playwright";
import { launch } from "./support/launch.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "../../..");
const previews = join(root, "docs/ui-data/previews");
const dist = join(root, "crates/navree-ui/dist");
const font = join(root, "crates/navree-ui/assets/fonts/geist.woff2");

const TYPES = { ".html": "text/html; charset=utf-8", ".css": "text/css", ".js": "text/javascript", ".woff2": "font/woff2" };

/** The preview documents as the site serves them: `../_toolkit/name-<hash>.ext` is `dist/name.ext`. */
function serve() {
  return createServer((request, response) => {
    const url = new URL(request.url, "http://x");
    const path = decodeURIComponent(url.pathname);
    let file = null;
    const toolkit = /^\/previews\/_toolkit\/(.+)-[0-9a-f]{12}\.(css|js|woff2)$/.exec(path);
    if (toolkit) file = toolkit[1] === "geist" ? font : join(dist, `${toolkit[1]}.${toolkit[2]}`);
    else if (path.startsWith("/previews/") && !path.includes("..")) file = join(previews, path.slice("/previews/".length));
    try {
      const body = readFileSync(file);
      const extension = file.slice(file.lastIndexOf("."));
      response.writeHead(200, { "content-type": TYPES[extension] ?? "application/octet-stream" });
      response.end(body);
    } catch {
      response.writeHead(404);
      response.end("not found");
    }
  });
}

/** The frame's width on a phone 390, 375 and 360 px wide. */
const FRAMES = [356, 341, 326];
/** The previews that draw a menu or a panel open in place. */
const MENUS = ["themes/menu.html", "navigation-menu/default.html", "navigation-menu/current.html"];
/** The galleries of date and time boxes: one that can be cleared has two buttons beside what it holds. */
const BOXES = ["date-picker/states.html", "date-picker/locales.html", "time-picker/states.html", "date-time-picker/states.html"];

const browsers = (process.env.PREVIEW_BROWSERS || "chromium firefox").split(" ");

for (const browserName of browsers) {
  describe(browserName, () => {
    let browser;
    let server;
    let base;
    before(async () => {
      browser = await launch(playwright[browserName]);
      server = serve();
      await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
      base = `http://127.0.0.1:${server.address().port}`;
    });
    after(async () => {
      await browser?.close();
      server?.close();
    });

    /** Opens a preview in a window as wide as the frame, and taller than any of these previews: nothing scrolls. */
    async function open(preview, width) {
      const context = await browser.newContext({ viewport: { width, height: 1000 }, reducedMotion: "reduce" });
      const page = await context.newPage();
      const problems = [];
      page.on("pageerror", (error) => problems.push(String(error)));
      page.on("console", (message) => {
        if (browserName === "webkit" && /Refused to apply a stylesheet/.test(message.text())) return;
        if (message.type() === "error" && !/Failed to load resource/.test(message.text())) problems.push(message.text());
      });
      await page.goto(`${base}/previews/${preview}`);
      // The toolkit's typeface, where it loads: what is measured is what is drawn.
      await page.evaluate(() => document.fonts.ready);
      return { page, problems, close: () => context.close() };
    }

    const twoFrames = (page) => page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
    /** The popups a preview draws in place: closed, and with a box all the same. */
    const pictures = (page) => page.evaluate(() => [...document.querySelectorAll("[popover]")].filter((el) => !el.matches(":popover-open") && el.getClientRects().length).map((el) => el.id));
    /** How far the frame scrolls sideways, in px: none when what it shows fits. */
    const sideways = (page) => page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);

    for (const width of FRAMES) {
      test(`a menu or a panel drawn open in place is no wider than what holds it, in a ${width} px frame`, async () => {
        for (const preview of MENUS) {
          const { page, problems, close } = await open(preview, width);
          const drawn = await page.evaluate(() => {
            const stage = document.querySelector("[data-preview-stage]").getBoundingClientRect();
            return {
              stage: [stage.left, stage.right],
              window: window.innerWidth,
              pictures: [...document.querySelectorAll("[popover]")]
                .filter((el) => el.getClientRects().length)
                .map((el) => {
                  const box = el.getBoundingClientRect();
                  return { id: el.id, left: box.left, right: box.right };
                }),
            };
          });
          assert.ok(drawn.pictures.length > 0, `${preview} draws a popup in place`);
          // What holds them is the stage: the frame less its padding, 24 px on each side.
          assert.ok(drawn.stage[0] >= 24 - 0.5 && drawn.stage[1] <= drawn.window - 24 + 0.5, `${preview}: the stage is at ${JSON.stringify(drawn.stage)} in a window ${drawn.window} px wide`);
          for (const picture of drawn.pictures) {
            assert.ok(picture.left >= drawn.stage[0] - 0.5 && picture.right <= drawn.stage[1] + 0.5, `${preview}: ${JSON.stringify(picture)} is drawn past what holds it, ${JSON.stringify(drawn.stage)}`);
          }
          assert.equal(await sideways(page), 0, `${preview} scrolls sideways`);
          assert.deepEqual(problems, [], preview);
          await close();
        }
      });

      test(`opened by its own button it keeps a margin from the edge of a ${width} px frame, and closed it is back in its place`, async () => {
        for (const preview of MENUS) {
          const { page, problems, close } = await open(preview, width);
          const ids = await pictures(page);
          assert.ok(ids.length > 0, `${preview} draws a popup in place`);
          for (const id of ids) {
            const place = () =>
              page.evaluate((popup) => {
                const box = document.getElementById(popup).getBoundingClientRect();
                return [box.left, box.right];
              }, id);
            const was = await place();
            await page.click(`[popovertarget="${id}"]`);
            // The browser's toggle event places it, a task after it shows.
            await page.waitForFunction((popup) => document.getElementById(popup).matches(":popover-open") && document.getElementById(popup).style.top !== "", id);
            await twoFrames(page);
            // In layout sizes: the popup is a little scaled while it fades in.
            const opened = await page.evaluate((popup) => {
              const pop = document.getElementById(popup);
              const left = parseFloat(pop.style.left);
              return { left, right: left + pop.offsetWidth, window: window.innerWidth };
            }, id);
            assert.ok(opened.left >= 8 - 0.5 && opened.right <= opened.window - 8 + 0.5, `${preview}, ${id}, open: a margin on each side of it: ${JSON.stringify(opened)}`);
            assert.equal(await sideways(page), 0, `${preview} scrolls sideways with ${id} open`);
            await page.keyboard.press("Escape");
            await page.waitForFunction((popup) => !document.getElementById(popup).matches(":popover-open"), id);
            await twoFrames(page);
            const now = await place();
            assert.ok(Math.abs(now[0] - was[0]) < 1 && Math.abs(now[1] - was[1]) < 1, `${preview}, ${id}, closed: drawn at ${JSON.stringify(now)}, and was at ${JSON.stringify(was)}`);
          }
          assert.equal(await sideways(page), 0, `${preview} scrolls sideways`);
          assert.deepEqual(problems, [], preview);
          await close();
        }
      });

      test(`a date or a time box in a gallery shows the whole of what it holds, in a ${width} px frame`, async () => {
        for (const preview of BOXES) {
          const { page, problems, close } = await open(preview, width);
          const read = await page.evaluate(() => {
            // A line of text in the page, out of sight, drawn in each box's own typeface in turn.
            const ruler = document.createElement("span");
            for (const [name, value] of [["position", "fixed"], ["left", "0"], ["top", "0"], ["visibility", "hidden"], ["white-space", "pre"]]) ruler.style.setProperty(name, value);
            document.body.append(ruler);
            const boxes = [...document.querySelectorAll("[data-preview-cell] [data-ui-text]")].filter((box) => box.getClientRects().length);
            const cut = [];
            for (const box of boxes) {
              const style = getComputedStyle(box);
              for (const name of ["font-family", "font-size", "font-weight", "font-style", "font-stretch", "font-variant-numeric", "font-feature-settings", "letter-spacing"]) ruler.style.setProperty(name, style.getPropertyValue(name));
              ruler.textContent = box.value;
              const text = ruler.getBoundingClientRect().width;
              // The box less its padding and border: an engine may or may not count a text box's padding in its clientWidth.
              const room = box.getBoundingClientRect().width - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight) - parseFloat(style.borderLeftWidth) - parseFloat(style.borderRightWidth);
              if (text > room + 0.5) cut.push(`"${box.value}" is ${text.toFixed(1)}px wide in ${room.toFixed(1)}px`);
            }
            ruler.remove();
            return { boxes: boxes.map((box) => box.value), cut };
          });
          assert.ok(read.boxes.length >= 3 && read.boxes.every((held) => held !== ""), `${preview} shows boxes that hold something: ${JSON.stringify(read.boxes)}`);
          assert.deepEqual(read.cut, [], `${preview} cuts what a box holds`);
          assert.equal(await sideways(page), 0, `${preview} scrolls sideways`);
          assert.deepEqual(problems, [], preview);
          await close();
        }
      });
    }

    test("the boxes that can be cleared are among them: two buttons beside what such a box holds", async () => {
      for (const preview of ["time-picker/states.html", "date-time-picker/states.html"]) {
        const { page, close } = await open(preview, FRAMES[0]);
        const most = await page.evaluate(() =>
          Math.max(...[...document.querySelectorAll("[data-preview-cell] [data-slot=input-group]")].map((group) => [...group.querySelectorAll("button")].filter((button) => button.getClientRects().length).length)),
        );
        assert.equal(most, 2, preview);
        await close();
      }
    });
  });
}
