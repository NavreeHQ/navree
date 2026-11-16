// The app shell's options in real browsers, against the proof app's `/shell/options`: a bare and
// dark mark, a badge in the brand row, a workspace switcher drawn as a two-line card, a slot above
// the person and the person's initials, actions in the top bar, a last crumb other than the
// title, badges beside the title and a subtitle under it, and a title in mono. Each is measured
// (sizes, places, colours and corners) in light and dark at 1440 and 390 px, with axe clean, no
// policy report and no sideways scroll; folding hides what must fold. Run with the proof app
// serving:
//
//   cargo run --release -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit npm test -- tests/shell.test.mjs
//
// UI_SHOTS=<dir> also saves a screenshot of every state.

import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { after, afterEach, before, describe, test } from "node:test";
import { chromium, firefox, webkit } from "playwright";
import { launch } from "./support/launch.mjs";

const BASE = process.env.UI_PROOF_URL || "http://127.0.0.1:8090";
const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");
const ENGINES = { chromium, firefox, webkit };
const NAMES = (process.env.UI_BROWSERS || "chromium").split(",").filter((n) => ENGINES[n]);
const SHOTS = process.env.UI_SHOTS;
if (SHOTS) mkdirSync(SHOTS, { recursive: true });

async function axe(page) {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const out = await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] });
    return out.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
  });
}

/** WCAG's ratio of two computed `rgb()` colours. */
const contrastOf = (a, b) => {
  const lum = (text) => {
    const [r, g, b] = text.match(/\d+(\.\d+)?/g).slice(0, 3).map(Number);
    const c = (v) => ((v /= 255) <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
    return 0.2126 * c(r) + 0.7152 * c(g) + 0.0722 * c(b);
  };
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
};

/** `a` is within half a pixel of `b`. */
const near = (a, b, what) => assert.ok(Math.abs(a - b) <= 0.5, `${what}: ${a} is not ${b}`);

/** The shell theme's muted text, in light and in dark. */
const MUTED = { light: "rgb(94, 94, 90)", dark: "rgb(183, 183, 178)" };

for (const name of NAMES) {
  describe(name, { concurrency: false }, () => {
    let browser;
    before(async () => {
      browser = await launch(ENGINES[name]);
    });
    after(async () => {
      await browser.close();
    });
    const contexts = [];
    afterEach(async () => {
      for (const context of contexts.splice(0)) await context.close().catch(() => {});
    });

    /**
     * A page on the proof app. Console errors, page errors and content security policy
     * reports are kept in `page.problems`; `page.done()` asserts there are none.
     */
    async function open(path, options = {}) {
      const { cookies, ...rest } = options;
      const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: "light", ...rest });
      contexts.push(context);
      if (cookies) await context.addCookies(cookies.map((c) => ({ ...c, url: BASE })));
      const page = await context.newPage();
      page.problems = [];
      page.on("console", (m) => {
        if (name === "webkit" && /Refused to apply a stylesheet/.test(m.text())) return;
        if (m.type() === "error" && !/Failed to load resource|404/.test(m.text())) page.problems.push(m.text());
      });
      page.on("pageerror", (e) => page.problems.push("pageerror: " + e.message));
      await page.addInitScript(() => {
        document.addEventListener("securitypolicyviolation", (e) => {
          (window.__csp ||= []).push(e.violatedDirective + " " + e.blockedURI);
        });
      });
      page.done = async () => {
        const csp = await page.evaluate(() => window.__csp || []);
        assert.deepEqual(csp, [], "no content security policy report");
        assert.deepEqual(page.problems, [], "no console or page error");
      };
      await page.goto(BASE + path, { waitUntil: "load" });
      return page;
    }

    const shot = async (page, file) => {
      if (!SHOTS) return;
      await page.screenshot({ path: join(SHOTS, `${name}-shell-${file}.png`) });
    };

    const noSidewaysScroll = (page) => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1);

    /** The boxes, colours, corners and type of what the shell's options draw. */
    const measure = (page) =>
      page.evaluate(() => {
        const one = (selector) => {
          const found = document.querySelector(selector);
          if (!found) throw new Error(`no ${selector}`);
          return found;
        };
        const box = (el) => {
          const r = el.getBoundingClientRect();
          return { x: r.x, y: r.y, w: r.width, h: r.height, bottom: r.bottom, right: r.right };
        };
        const css = (el) => getComputedStyle(el);
        const token = (name) => {
          const probe = document.createElement("span");
          document.body.append(probe);
          probe.style.color = `var(--${name})`;
          const out = getComputedStyle(probe).color;
          probe.remove();
          return out;
        };
        const font = (name) => {
          const probe = document.createElement("span");
          document.body.append(probe);
          probe.style.fontFamily = `var(--${name})`;
          const out = getComputedStyle(probe).fontFamily;
          probe.remove();
          return out;
        };
        const brand = one("[data-slot=sidebar] [data-slot=sidebar-brand]");
        const logo = one("[data-slot=sidebar] [data-slot=sidebar-logo]");
        const mark = logo.querySelector("img");
        const name = one("[data-slot=sidebar] [data-slot=sidebar-brand-name]");
        const badge = one("[data-slot=sidebar] [data-slot=sidebar-brand] > [data-slot=badge]");
        const card = one("[data-slot=switcher-card]");
        const tile = one("[data-slot=switcher-card] [data-slot=avatar]");
        const cardName = one("[data-slot=switcher-name]");
        const cardLine = one("[data-slot=switcher-description]");
        const chevron = one("[data-slot=switcher-card] > [data-slot=icon]");
        const slot = one("[data-slot=sidebar-footer-slot]");
        const user = one("[data-slot=sidebar] [data-slot=sidebar-user]");
        const avatar = one("[data-slot=sidebar] [data-slot=sidebar-user] [data-slot=avatar]");
        const userName = one("[data-slot=sidebar] [data-slot=sidebar-user-name]");
        const userLine = one("[data-slot=sidebar] [data-slot=sidebar-user-email]");
        const topbar = one("[data-slot=topbar]");
        const trigger = one("[data-ui-sidebar-trigger]");
        const separator = one("[data-slot=topbar] [data-slot=separator]");
        const actions = one("[data-slot=topbar-actions]");
        const crumbs = one("[data-slot=topbar] [data-slot=breadcrumb]");
        const crumbLink = crumbs.querySelector("a");
        const crumbHere = crumbs.querySelector("[aria-current=page]");
        const title = one("[data-slot=page-title]");
        const aside = one("[data-slot=page-heading] > [data-slot=badge]");
        const subtitle = one("[data-slot=page-subtitle]");
        return {
          accent: token("accent"),
          accentInk: token("accent-ink"),
          mono: font("font-mono"),
          brand: { ...box(brand), bg: css(logo).backgroundColor },
          mark: mark ? { ...box(mark), src: mark.currentSrc || mark.src } : null,
          name: { ...box(name), size: css(name).fontSize, weight: css(name).fontWeight },
          badge: { ...box(badge), radius: css(badge).borderTopLeftRadius, bg: css(badge).backgroundColor, color: css(badge).color, display: css(badge).display },
          card: {
            ...box(card),
            padding: css(card).paddingTop,
            border: [css(card).borderTopWidth, css(card).borderTopColor],
            radius: css(card).borderTopLeftRadius,
            bg: css(card).backgroundColor,
            text: css(card).textAlign,
          },
          tile: { ...box(tile), radius: css(tile).borderTopLeftRadius, bg: css(tile).backgroundColor, color: css(tile).color },
          cardName: { ...box(cardName), size: css(cardName).fontSize, weight: css(cardName).fontWeight, overflow: css(cardName).textOverflow },
          cardLine: { ...box(cardLine), size: css(cardLine).fontSize, color: css(cardLine).color },
          chevron: { ...box(chevron), color: css(chevron).color },
          slot: { ...box(slot), display: css(slot).display },
          user: { ...box(user), chevrons: user.querySelectorAll(":scope > [data-slot=icon]").length },
          avatar: { ...box(avatar), radius: css(avatar).borderTopLeftRadius, size: css(avatar).fontSize, weight: css(avatar).fontWeight, text: avatar.textContent },
          userName: { size: css(userName).fontSize, weight: css(userName).fontWeight },
          userLine: { color: css(userLine).color },
          topbar: { ...box(topbar), padding: css(topbar).paddingLeft },
          trigger: css(trigger).display,
          separator: css(separator).display,
          actions: { ...box(actions), inBar: actions.closest("[data-slot=topbar]") === topbar },
          crumbs: {
            size: css(crumbs).fontSize,
            link: css(crumbLink).fontWeight,
            here: [css(crumbHere).fontWeight, css(crumbHere).fontFamily, crumbHere.textContent],
          },
          title: { ...box(title), size: css(title).fontSize, family: css(title).fontFamily },
          aside: box(aside),
          subtitle: { ...box(subtitle), color: css(subtitle).color, size: css(subtitle).fontSize },
          sidebar: { ...box(one("[data-slot=sidebar]")), bottom: one("[data-slot=sidebar]").getBoundingClientRect().bottom },
        };
      });

    for (const mode of ["light", "dark"]) {
      test(`${mode}, 1440 px: the brand row, the card switcher, the foot, the top bar and the page header are drawn as the theme and the options say`, async () => {
        const page = await open(`/shell/options?mode=${mode}`, { colorScheme: mode });
        const m = await measure(page);
        // The brand row: a bare 22 px mark at 20, 20, no tile, 10 px before the name; 30 px high.
        assert.equal(m.brand.h, 30, "the brand row's height");
        near(m.brand.x, 12, "the brand row's place");
        assert.equal(m.brand.bg, "rgba(0, 0, 0, 0)", "no tile behind a bare mark");
        assert.deepEqual([m.mark.w, m.mark.h], [22, 22], "the mark's size");
        near(m.mark.x, 20, "the mark's left");
        near(m.mark.y, 20, "the mark's top");
        near(m.name.x, 52, "the wordmark's left");
        assert.equal(m.name.size, "18px", "the wordmark's size");
        assert.equal(m.name.weight, "600", "the wordmark's weight");
        // The dark mark on a dark page, the light one on a light page.
        if (mode === "dark") assert.match(m.mark.src, /proof-mark-dark-/, "the dark mark");
        else assert.doesNotMatch(m.mark.src, /proof-mark-dark-/, "the light mark");
        // The badge: 22 px, a pill in the accent, at the row's end.
        assert.equal(m.badge.h, 22, "the badge's height");
        assert.equal(m.badge.radius, "999px", "a pill");
        assert.deepEqual([m.badge.bg, m.badge.color], [m.accent, m.accentInk], "the accent and its ink");
        assert.ok(contrastOf(m.badge.bg, m.badge.color) >= 4.5, "the badge reads");
        near(m.badge.right, m.brand.right - 8, "the badge at the row's end");
        // The switcher: a card as wide as its place at 12, 64, padded 8, a 1 px line, 12 px
        // corners, white.
        near(m.card.x, 12, "the card's left");
        near(m.card.y, 64, "the card's top: one 18 px gap under the brand row");
        // As wide as the sidebar's inside: its 248 px less its 1 px edge and two 12 px insets.
        near(m.card.w, m.sidebar.w - 1 - 24, "the card's width");
        assert.equal(m.card.padding, "8px", "the card's padding");
        assert.equal(m.card.border[0], "1px", "the card's line");
        assert.equal(m.card.radius, "12px", "the field corner");
        assert.equal(m.card.bg, mode === "light" ? "rgb(255, 255, 255)" : "rgb(18, 18, 18)", "the card colour");
        assert.equal(m.card.text, "start", "its text starts at the start");
        // Its tile: the organisation's initials, filled with the accent, the avatar corner.
        assert.deepEqual([m.tile.w, m.tile.h], [32, 32], "the tile's size");
        assert.equal(m.tile.radius, "8px", "the tile's corner");
        assert.deepEqual([m.tile.bg, m.tile.color], [m.accent, m.accentInk], "the tile's fill");
        near(m.cardName.x, m.tile.right + 10, "10 px between the tile and the name");
        assert.deepEqual([m.cardName.size, m.cardName.weight, m.cardName.overflow], ["13px", "500", "ellipsis"], "the name");
        assert.deepEqual([m.cardLine.size, m.cardLine.color], ["12px", MUTED[mode]], "the second line");
        assert.ok(m.cardLine.y >= m.cardName.bottom - 0.5, "the second line under the name");
        assert.deepEqual([m.chevron.w, m.chevron.color], [16, MUTED[mode]], "the chevron");
        near(m.chevron.right, m.card.right - 9, "the chevron at the end");
        // The foot: the slot 12 px above the person; the person's row as wide at 12, 16 px
        // above the sidebar's end; a round avatar of initials and no chevron.
        near(m.user.y - m.slot.bottom, 12, "12 px between the slot and the person");
        near(m.user.x, 12, "the person's left");
        near(m.user.w, m.sidebar.w - 1 - 24, "the person's width");
        near(m.sidebar.bottom - m.user.bottom, 16, "the sidebar's inset under the person");
        assert.equal(m.user.chevrons, 0, "no chevron");
        assert.equal(m.avatar.radius, "50%", "a round avatar");
        assert.deepEqual([m.avatar.text, m.avatar.size, m.avatar.weight], ["DO", "12px", "600"], "the initials");
        assert.deepEqual([m.userName.size, m.userName.weight], ["13px", "500"], "the person's name");
        assert.equal(m.userLine.color, MUTED[mode], "the person's second line");
        // The top bar: 60 px, 32 px at its sides; no fold button and no separator; the page's
        // actions in it; a 14 px trail whose last part is 500 and differs from the title.
        assert.equal(m.topbar.h, 60, "the top bar's height");
        assert.equal(m.topbar.padding, "32px", "the top bar's side");
        assert.deepEqual([m.trigger, m.separator], ["none", "none"], "no fold button and no separator");
        assert.ok(m.actions.inBar, "the actions in the top bar");
        near(m.actions.right, m.topbar.right - 32 + 4, "the actions at the bar's end");
        assert.equal(m.crumbs.size, "14px", "the trail's size");
        assert.deepEqual([m.crumbs.link, m.crumbs.here[0], m.crumbs.here[2]], ["400", "500", "LDH-1"], "the trail's weights and last part");
        // The page header: badges on the title's line, a muted 14 px line 6 px under it.
        assert.equal(m.title.size, "28px", "the title");
        assert.ok(m.aside.y >= m.title.y - 1 && m.aside.bottom <= m.title.bottom + 1, "the badges on the title's line");
        assert.ok(m.aside.x > m.title.right, "the badges after the title");
        near(m.subtitle.y - m.title.bottom, 6, "6 px between the title and the subtitle");
        assert.deepEqual([m.subtitle.color, m.subtitle.size], [MUTED[mode], "14px"], "the subtitle");
        assert.ok(await noSidewaysScroll(page), "no sideways scroll");
        assert.deepEqual(await axe(page), [], "axe");
        await shot(page, `${mode}-1440`);
        // The switcher's menu: each workspace with its tile, the current one's tick at the end.
        await page.click("[data-slot=switcher-card]");
        await page.waitForSelector("#ui-workspace-switcher:popover-open");
        // Read once the menu has eased in: no transition still running on what is drawn.
        await page.waitForFunction(() => !document.getAnimations().some((a) => a.playState === "running" && a.effect?.target?.checkVisibility?.()));
        const rows = await page.evaluate(() =>
          [...document.querySelectorAll("#ui-workspace-switcher [data-slot=menu-link]")].map((row) => [
            row.querySelector("[data-slot=avatar]")?.getBoundingClientRect().x ?? -1,
            row.lastElementChild?.getAttribute("data-slot"),
          ]),
        );
        assert.equal(rows.length, 2, "two workspaces");
        near(rows[0][0], rows[1][0], "their tiles line up");
        assert.equal(rows[0][1], "icon", "the current one's tick at the end");
        assert.deepEqual(await axe(page), [], "axe with the menu open");
        await shot(page, `${mode}-menu`);
        await page.done();
      });

      test(`${mode}, 390 px: the options fit a phone, the actions scroll inside the bar and the page never sideways`, async () => {
        const page = await open(`/shell/options?mode=${mode}`, { colorScheme: mode, viewport: { width: 390, height: 844 } });
        const seen = await page.evaluate(() => {
          const r = (s) => document.querySelector(s).getBoundingClientRect();
          const css = (s) => getComputedStyle(document.querySelector(s));
          const actions = document.querySelector("[data-slot=topbar-actions]");
          return {
            trigger: css("[data-ui-sidebar-trigger]").display,
            separator: css("[data-slot=topbar] [data-slot=separator]").display,
            actions: [r("[data-slot=topbar-actions]").right, actions.scrollWidth > actions.clientWidth],
            title: r("[data-slot=page-title]").right,
            subtitle: css("[data-slot=page-subtitle]").display,
          };
        });
        assert.notEqual(seen.trigger, "none", "the button that opens the sidebar stays on a phone");
        assert.notEqual(seen.separator, "none", "and its separator with it");
        assert.ok(seen.actions[0] <= 390, `the actions inside the window: ${seen.actions[0]}`);
        assert.ok(seen.title <= 390, "the title inside the window");
        assert.ok(await noSidewaysScroll(page), "no sideways scroll");
        assert.deepEqual(await axe(page), [], "axe");
        await shot(page, `${mode}-390`);
        // The sidebar's sheet: the brand row, the card and the foot as on a wide screen.
        await page.click("[data-ui-sidebar-trigger]");
        await page.waitForFunction(() => document.querySelector("[data-slot=sidebar-wrapper]").hasAttribute("data-mobile-open"));
        await page.waitForTimeout(250);
        const sheet = await page.evaluate(() => {
          const r = (s) => document.querySelector(s).getBoundingClientRect();
          return { card: r("[data-slot=switcher-card]"), badge: r("[data-slot=sidebar-brand] > [data-slot=badge]"), slot: r("[data-slot=sidebar-footer-slot]") };
        });
        assert.ok(sheet.card.width > 200 && sheet.badge.width > 0 && sheet.slot.height > 0, "the sheet draws them");
        assert.deepEqual(await axe(page), [], "axe with the sheet open");
        await shot(page, `${mode}-390-sheet`);
        await page.done();
      });
    }

    test("a page that follows the device draws the dark mark when the device is dark", async () => {
      for (const scheme of ["light", "dark"]) {
        const page = await open("/shell/options?mode=system", { colorScheme: scheme });
        const src = await page.evaluate(() => {
          const img = document.querySelector("[data-slot=sidebar] [data-slot=sidebar-logo] picture img");
          return img.currentSrc;
        });
        if (scheme === "dark") assert.match(src, /proof-mark-dark-/, "the dark source");
        else assert.doesNotMatch(src, /proof-mark-dark-/, "the light source");
        const box = await page.evaluate(() => document.querySelector("[data-slot=sidebar] [data-slot=sidebar-logo] img").getBoundingClientRect().width);
        assert.equal(box, 22, "the picture's mark is the bare mark's size");
        await page.done();
      }
    });

    test("a title and a last crumb in mono", async () => {
      const page = await open("/shell/options?mono");
      const m = await measure(page);
      assert.equal(m.title.family, m.mono, "the title in the mono face");
      assert.equal(m.crumbs.here[1], m.mono, "the last crumb in the mono face");
      assert.equal(m.crumbs.here[2], "INV-2041");
      assert.deepEqual(await axe(page), [], "axe");
      await page.done();
    });

    test("folded to its icons, the sidebar hides the badge, the card and the slot at its foot", async () => {
      const page = await open("/shell/options?icons", { cookies: [{ name: "navree_ui_sidebar", value: "collapsed" }] });
      const seen = await page.evaluate(() => {
        const css = (s) => getComputedStyle(document.querySelector(s)).display;
        return {
          folded: document.querySelector("[data-slot=sidebar-wrapper]").hasAttribute("data-collapsed"),
          width: document.querySelector("[data-slot=sidebar]").getBoundingClientRect().width,
          badge: css("[data-slot=sidebar-brand] > [data-slot=badge]"),
          slot: css("[data-slot=sidebar-header-slot]"),
          foot: css("[data-slot=sidebar-footer-slot]"),
          avatar: document.querySelector("[data-slot=sidebar-user] [data-slot=avatar]").getBoundingClientRect().width,
          mark: document.querySelector("[data-slot=sidebar-logo] img").getBoundingClientRect().width,
        };
      });
      assert.ok(seen.folded, "folded");
      assert.deepEqual([seen.badge, seen.slot, seen.foot], ["none", "none", "none"], "the badge, the switcher and the slot fold away");
      assert.equal(seen.avatar, 32, "the person's avatar stays");
      assert.equal(seen.mark, 22, "the bare mark stays");
      assert.deepEqual(await axe(page), [], "axe");
      await shot(page, "folded");
      await page.done();
    });
  });
}
