/**
 * Opens every overlay in every UI preview of the built docs, at a desktop and a
 * phone width, and fails when:
 *
 * - an open popup, list, picker or dialog is cut by its frame, covers the
 *   control that opened it, or shows fewer than five of its rows (or all of
 *   them, when it has fewer);
 * - the frame shrinks while something is open, the control moves under the
 *   pointer, or the frame doesn't go back to its height once it closes;
 * - a hover card or tooltip opens more than once while the pointer rests;
 * - a preview logs an error, asks for an address that answers 4xx or 5xx, or
 *   is navigated away from its own page;
 * - one of the checks of a single preview fails: the theme builder is styled,
 *   drawn in its draft at the column's width and says what needs a server; the
 *   theme menu applies a theme; the narrow toolbar shows More; the lazy lists
 *   load their next page.
 *
 * Run after `pnpm build`:
 *
 *   pnpm check:previews                                   serves out/ itself
 *   PAGES=/docs/components/select/ pnpm check:previews    some pages only
 *   VIEWPORTS=1440x900 pnpm check:previews                one width only
 *   BASE_URL=http://127.0.0.1:3211 pnpm check:previews    a server already running
 *   pnpm check:previews --measure                         also writes the height each preview
 *                                                         settles at, at 1440x900, to
 *                                                         src/components/ui-docs/heights.json
 *
 * After `--measure`, build again: the frames then start at the height they settle at.
 */
import { spawn, type ChildProcess } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";

import { chromium, type Frame, type Locator, type Page } from "playwright";

import { OUT, WEBSITE, htmlFiles, requireOut } from "./lib/site-html.ts";

const MEASURE = process.argv.includes("--measure");
const HEIGHTS = path.join(WEBSITE, "src/components/ui-docs/heights.json");

/** What opens something: a popover's button, a dialog's, a select, a combobox, a multi-select. */
const OPENERS = [
  "[popovertarget]",
  "[command=show-modal][commandfor]",
  "button[data-slot=select-trigger]",
  "input[data-slot=combobox-input]",
  "input[data-slot=multiselect-input]",
].join(", ");
const MOST_OPENERS = 12;
const MOST_HOVERS = 4;
const MOST_SUBMITS = 6;
/** How far a frame or a control may move, in px, before it counts as a jump. */
const JUMP = 4;
/** The fewest rows a list must show, unless it has fewer. */
const ROWS = 5;
/** Errors that are not the previews': the CDN's analytics beacon, which only the live site adds. */
const ALLOWED = [/cloudflareinsights/];

const viewports = (process.env.VIEWPORTS ?? "1440x900,390x844")
  .split(",")
  .map((size) => size.trim().split("x").map(Number))
  .filter(([width, height]) => width > 0 && height > 0)
  .map(([width, height]) => ({ width, height }));

/** Every docs page that shows previews: the components and the UI guides. */
function docsPages(): string[] {
  return htmlFiles()
    .map((file) => `/${path.relative(OUT, file).split(path.sep).join("/")}`.replace(/index\.html$/, ""))
    .filter((url) => url.endsWith("/") && (url.startsWith("/docs/components/") || url.startsWith("/docs/ui/")))
    .sort();
}

const ready = (url: string) =>
  new Promise<boolean>((resolve) => {
    http
      .get(url, (res) => {
        res.resume();
        resolve((res.statusCode ?? 500) < 500);
      })
      .on("error", () => resolve(false));
  });

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

type Shown = {
  slot: string;
  inside: boolean;
  covers: boolean;
  rows: number;
  seen: number;
};

/** What is open in a preview now, read in its frame: where each is, and how many rows it shows. */
function shownIn(frame: Frame, opener: Locator | null): Promise<Shown[]> {
  const read = (target: Element | null): Shown[] => {
    const passedOver = "[data-preview-note], [data-slot=toaster]";
    let open: Element[] = [];
    try {
      open = [...document.querySelectorAll(":popover-open, dialog:modal")].filter((el) => !el.matches(passedOver));
    } catch {
      return [];
    }
    const at = target?.getBoundingClientRect();
    return open.map((el) => {
      const box = el.getBoundingClientRect();
      const rows = [...el.querySelectorAll("[role=option],[role=menuitem],[role=menuitemradio],[role=menuitemcheckbox]")].filter(
        (row) => row.getClientRects().length > 0,
      );
      const seen = rows.filter((row) => {
        const r = row.getBoundingClientRect();
        return r.height > 0 && r.top >= Math.max(box.top, 0) - 1 && r.bottom <= Math.min(box.bottom, innerHeight) + 1;
      }).length;
      const id = el.id ? CSS.escape(el.id) : "";
      const anchor = id ? document.querySelector(`[popovertarget="${id}"],[data-ui-hover="${id}"],[data-ui-anchor="${id}"]`) : null;
      const mine = !!target && !!anchor && (anchor === target || anchor.contains(target));
      const apart = !at || box.bottom <= at.top + 1 || box.top >= at.bottom - 1 || box.right <= at.left + 1 || box.left >= at.right - 1;
      return {
        slot: el.getAttribute("data-slot") || el.localName,
        inside: box.top >= -1 && box.left >= -1 && box.bottom <= innerHeight + 1 && box.right <= innerWidth + 1,
        covers: mine && !apart,
        rows: rows.length,
        seen,
      };
    });
  };
  return opener ? opener.evaluate(read) : frame.evaluate(() => read(null));
}

/** The frame's own height, in its px: unscaled, as the page inside it sees it. */
const heightOf = (frame: Frame) => frame.evaluate(() => innerHeight);
const topOf = (target: Locator) => target.evaluate((el) => el.getBoundingClientRect().top);

/** Closes whatever is open in the frame, and waits for the frame to settle. */
async function closeAll(page: Page, frame: Frame) {
  await page.keyboard.press("Escape").catch(() => {});
  await frame
    .evaluate(() => {
      for (const el of document.querySelectorAll(":popover-open")) if (!el.matches("[data-slot=toaster]")) (el as HTMLElement).hidePopover();
      for (const dialog of document.querySelectorAll("dialog[open]")) (dialog as HTMLDialogElement).close();
    })
    .catch(() => {});
  // The frame keeps its room 150 ms after the last popup closes, then goes back.
  await wait(500);
}

/** The checks of one preview, by its id: what only that preview must do. */
const own: Record<string, (page: Page, frame: Frame, iframe: Locator, fail: (what: string) => void) => Promise<void>> = {
  "theme-builder/default": async (_page, frame, iframe, fail) => {
    const styled = await frame.evaluate(() => {
      const choice = document.querySelector("[data-slot=theme-choice]");
      return !!choice && Number.parseFloat(getComputedStyle(choice).paddingTop) > 0;
    });
    if (!styled) fail("the builder is not styled: its stylesheet is not linked");
    const look = await frame.evaluate(() => [document.documentElement.dataset.theme, document.documentElement.dataset.mode].join(" "));
    if (look !== "ocean dark") fail(`the page is drawn in "${look}", not in its draft, ocean dark`);
    const drawn = await iframe.evaluate((el) => ({
      shown: el.getBoundingClientRect().width,
      own: (el as HTMLIFrameElement).contentWindow?.innerWidth ?? 0,
      column: el.parentElement?.clientWidth ?? 0,
    }));
    if (Math.abs(drawn.shown - drawn.own) > 1) fail(`the builder is scaled: ${drawn.own} px drawn in ${Math.round(drawn.shown)} px`);
    if (drawn.shown < drawn.column * 0.95) fail(`the builder is ${Math.round(drawn.shown)} px wide in a ${drawn.column} px column`);
    const at = frame.url();
    await frame.locator("label[data-slot=theme-choice]", { hasText: "Violet" }).first().click();
    await wait(300);
    const said = await frame.evaluate(() => {
      const note = document.querySelector("[data-preview-note]");
      return note?.matches(":popover-open") ? (note.textContent ?? "") : "";
    });
    if (!said.includes("/settings/theme?")) fail(`choosing an accent says nothing about the server it needs ("${said}")`);
    if (frame.url() !== at) fail(`choosing an accent sent the frame to ${frame.url()}`);
  },
  "themes/menu": async (_page, frame, _iframe, fail) => {
    await frame.locator("[popovertarget][aria-label=Appearance]").first().click();
    await wait(300);
    await frame.locator(":popover-open label[data-slot=theme-card]", { hasText: "Violet" }).first().click();
    await frame.locator(":popover-open [data-theme-apply]").first().click();
    await wait(200);
    const theme = await frame.evaluate(() => document.documentElement.dataset.theme);
    if (theme !== "violet") fail(`choosing Violet and Apply leaves the theme "${theme}"`);
  },
  "toolbar/overflow": async (_page, frame, _iframe, fail) => {
    if (!(await frame.locator("[data-slot=toolbar-more]").first().isVisible())) fail("More is not shown at a narrow width");
  },
  "lazy-list/default": async (_page, frame, _iframe, fail) => {
    await wait(800);
    const items = await frame.locator("[data-slot=lazy-items] > li[data-key]").count();
    if (items <= 6) fail(`the list has ${items} items: its next page did not load`);
    if (await frame.locator("[data-slot=lazy-more][data-failed]").count()) fail("the list shows that a page failed to load");
  },
  "lazy-list/conversation": async (_page, frame, _iframe, fail) => {
    await wait(800);
    const items = await frame.locator("[data-slot=lazy-items] > li[data-key]").count();
    if (items <= 3) fail(`the conversation has ${items} messages: its earlier page did not load`);
    if (await frame.locator("[data-slot=lazy-more][data-failed]").count()) fail("the list shows that a page failed to load");
  },
};

async function main() {
  let server: ChildProcess | null = null;
  let base = process.env.BASE_URL ?? "";
  if (!base) {
    requireOut("check-previews");
    const port = String(30000 + Math.floor(Math.random() * 20000));
    base = `http://127.0.0.1:${port}`;
    server = spawn(process.execPath, [path.join(WEBSITE, "scripts", "serve.ts")], { env: { ...process.env, PORT: port }, stdio: "ignore" });
    for (let i = 0; i < 50 && !(await ready(`${base}/`)); i++) await wait(200);
  }
  const pages = process.env.PAGES
    ? process.env.PAGES.split(",")
        .map((p) => p.trim())
        .filter(Boolean)
    : docsPages();

  const problems: string[] = [];
  const heights: Record<string, number> = {};
  let frames = 0;
  let opened = 0;
  const browser = await chromium.launch();
  try {
    for (const viewport of viewports) {
      const size = `${viewport.width}x${viewport.height}`;
      const context = await browser.newContext({ viewport });
      const page = await context.newPage();
      let where = "";
      const fail = (what: string) => problems.push(`${size} ${where}: ${what}`);
      page.on("pageerror", (error) => fail(`uncaught: ${error.message.split("\n")[0]}`));
      page.on("console", (message) => {
        const text = message.text();
        if (message.type() === "error" && !ALLOWED.some((allowed) => allowed.test(text))) fail(`console: ${text.split("\n")[0]}`);
      });
      page.on("response", (response) => {
        const url = response.url();
        if (response.status() >= 400 && !ALLOWED.some((allowed) => allowed.test(url))) fail(`${response.status()} for ${url}`);
      });
      for (const url of pages) {
        where = url;
        await page.goto(`${base}${url}`, { waitUntil: "load" });
        const iframes = page.locator(".ui-preview iframe");
        const count = await iframes.count();
        for (let i = 0; i < count; i++) {
          const iframe = iframes.nth(i);
          await iframe.scrollIntoViewIfNeeded();
          // A deferred frame asks for its page as it nears the screen.
          await page.waitForFunction((el) => !!el?.getAttribute("src"), await iframe.elementHandle(), { timeout: 8000 }).catch(() => {});
          const src = (await iframe.getAttribute("src")) ?? "";
          const id = src.replace(/^.*\/ui-preview\//, "").replace(/\.html$/, "");
          where = `${url} ${id}`;
          const frame = await (await iframe.elementHandle())?.contentFrame();
          if (!frame || !src) {
            fail("the frame has no page");
            continue;
          }
          try {
            await frame.waitForSelector("[data-preview-stage]", { timeout: 8000 });
          } catch {
            fail("the preview did not load");
            continue;
          }
          await frame.evaluate(() => document.fonts.ready);
          await wait(400);
          frames++;
          const home = frame.url();
          if (MEASURE && viewport === viewports[0] && !(id in heights)) heights[id] = await heightOf(frame);

          // Every opener, one at a time.
          const openers = frame.locator(OPENERS);
          const many = Math.min(await openers.count(), MOST_OPENERS);
          for (let k = 0; k < many; k++) {
            const opener = openers.nth(k);
            if (!(await opener.isVisible()) || !(await opener.isEnabled())) continue;
            const name = await opener.evaluate(
              (el) => `${el.getAttribute("data-slot") || el.localName} "${(el.getAttribute("aria-label") || el.textContent || "").trim().slice(0, 30)}"`,
            );
            const before = await heightOf(frame);
            const top = await topOf(opener);
            try {
              await opener.click({ timeout: 2500 });
            } catch {
              console.warn(`check-previews: ${size} ${where}: ${name} could not be clicked`);
              continue;
            }
            // The popup opens, the frame grows to it, the toolkit places it again.
            await wait(600);
            const shown = await shownIn(frame, opener);
            if (shown.length) opened++;
            for (const one of shown) {
              const what = `${name} opens ${one.slot}`;
              if (!one.inside) fail(`${what}, cut by the frame`);
              if (one.covers) fail(`${what}, over its own control`);
              if (one.rows && one.seen < Math.min(one.rows, ROWS)) fail(`${what}, showing ${one.seen} of ${one.rows} rows`);
            }
            const during = await heightOf(frame);
            if (during < before - JUMP) fail(`${name}: the frame shrinks from ${before} to ${during} px while it is open`);
            const moved = Math.abs((await topOf(opener)) - top);
            if (moved > JUMP) fail(`${name}: the control moves ${Math.round(moved)} px as it opens`);
            await closeAll(page, frame);
            const after = await heightOf(frame);
            if (Math.abs(after - before) > JUMP) fail(`${name}: the frame is ${after} px after closing, ${before} px before`);
          }

          // Hover cards and tooltips stay open while the pointer rests.
          const hovers = frame.locator("[data-ui-hover]");
          const hovered = Math.min(await hovers.count(), MOST_HOVERS);
          for (let k = 0; k < hovered; k++) {
            const trigger = hovers.nth(k);
            if (!(await trigger.isVisible())) continue;
            await frame.evaluate(() => {
              const state = window as unknown as { previewOpens: number };
              state.previewOpens = 0;
              document.addEventListener("toggle", (event) => {
                if ((event as ToggleEvent).newState === "open") state.previewOpens++;
              }, true);
            });
            await trigger.hover();
            await wait(2000);
            const { opens, open } = await trigger.evaluate((el) => {
              const card = document.getElementById(el.getAttribute("data-ui-hover") ?? "");
              return { opens: (window as unknown as { previewOpens: number }).previewOpens, open: !!card?.matches(":popover-open") };
            });
            if (opens > 1 || !open) fail(`hover ${k}: opened ${opens} times in 2 s and is ${open ? "open" : "closed"}`);
            await page.mouse.move(0, 0);
            await closeAll(page, frame);
          }

          // A submit says what it would send; nothing is posted and the frame stays.
          const submits = frame.locator("button[type=submit], input[type=submit]");
          const sent = Math.min(await submits.count(), MOST_SUBMITS);
          for (let k = 0; k < sent; k++) {
            const submit = submits.nth(k);
            if (!(await submit.isVisible()) || !(await submit.isEnabled())) continue;
            await submit.click({ timeout: 2500 }).catch(() => {});
            await wait(200);
            await closeAll(page, frame);
          }

          const check = own[id];
          if (check) {
            try {
              await check(page, frame, iframe, fail);
            } catch (error) {
              fail(`its own check failed: ${String(error).split("\n")[0]}`);
            }
            await closeAll(page, frame);
          }
          if (frame.url() !== home) fail(`the frame was sent to ${frame.url()}`);
          where = url;
        }
      }
      await context.close();
    }
  } finally {
    await browser.close();
    server?.kill();
  }

  if (MEASURE) {
    const sorted = Object.fromEntries(Object.entries(heights).sort(([a], [b]) => a.localeCompare(b)));
    const file = {
      $comment: `Written by \`pnpm check:previews --measure\`: the height each preview settles at in the docs column at ${viewports[0]?.width ?? 1440} px. A frame starts at it, so the page doesn't shift as the frames load.`,
      heights: sorted,
    };
    fs.writeFileSync(HEIGHTS, `${JSON.stringify(file, null, 2)}\n`);
    console.log(`check-previews: wrote ${Object.keys(sorted).length} heights to ${path.relative(WEBSITE, HEIGHTS)}`);
  }
  if (problems.length) {
    console.error(`check-previews: ${problems.length} problem(s):\n- ${problems.join("\n- ")}`);
    process.exit(1);
  }
  console.log(
    `check-previews: ${pages.length} pages, ${frames} frames at ${viewports.map((v) => `${v.width}x${v.height}`).join(" and ")}: ${opened} overlays opened, each whole, with no errors`,
  );
}

await main();
