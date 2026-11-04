#!/usr/bin/env node
// Screenshots of every screen, dialog and state at 1440 and 390 px, for design review.
//
//   node scripts/screenshots.mjs [baseURL] [outDir]
//   SHOTS=login,dashboard node scripts/screenshots.mjs   # only some screens
//   CRM_USER=manager@navree.test node scripts/screenshots.mjs
//
// Defaults: http://localhost:3102, ./screenshots (git-ignored), signed in as admin@navree.test.
// Needs the app (`pnpm dev`) and a crm-api seeded with crm-demo (`pnpm api:fresh`).
// The names match apps/crm-web's script, so the two sets can be compared side by side.
// Console errors and page errors are printed.
import { mkdirSync } from "node:fs";
import path from "node:path";
import { chromium } from "@playwright/test";

const baseURL = process.argv[2] ?? process.env.CRM_URL ?? "http://localhost:3102";
const outDir = path.resolve(process.argv[3] ?? "screenshots");
const only = process.env.SHOTS ? new Set(process.env.SHOTS.split(",")) : null;
const USER = process.env.CRM_USER ?? "admin@navree.test";
const PASSWORD = "password123";

const VIEWPORTS = [
  { name: "1440", width: 1440, height: 900 },
  { name: "390", width: 390, height: 844, isMobile: true, hasTouch: true, deviceScaleFactor: 2 },
];

mkdirSync(outDir, { recursive: true });

/** A same-origin JSON call through the BFF, as the signed-in browser. */
async function api(page, method, url, body) {
  return page.evaluate(
    async ({ method, url, body }) => {
      const r = await fetch(url, {
        method,
        headers: { Accept: "application/json", ...(body ? { "Content-Type": "application/json" } : {}) },
        body: body ? JSON.stringify(body) : undefined,
      });
      return r.json();
    },
    { method, url, body },
  );
}

async function signIn(page, email) {
  await page.goto(`${baseURL}/login`);
  await page.getByLabel("Email").fill(email);
  await page.getByLabel("Password").fill(PASSWORD);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await page.waitForURL(`${baseURL}/`);
  await page.getByRole("heading", { level: 1, name: "Dashboard" }).waitFor();
}

async function settle(page) {
  await page.waitForLoadState("networkidle").catch(() => {});
  await page
    .waitForFunction(() => !document.querySelector('[aria-busy="true"]'), null, { timeout: 15_000 })
    .catch(() => {});
  await page.waitForTimeout(400);
}

async function shot(page, name, viewport, { fullPage = true } = {}) {
  const file = path.join(outDir, `${name}-${viewport.name}.png`);
  await page.screenshot({ path: file, fullPage });
  console.log(file);
}

const browser = await chromium.launch();
try {
  for (const viewport of VIEWPORTS) {
    const context = await browser.newContext({
      viewport: { width: viewport.width, height: viewport.height },
      isMobile: viewport.isMobile,
      hasTouch: viewport.hasTouch,
      deviceScaleFactor: viewport.deviceScaleFactor ?? 1,
      // crm-api's day boundaries ("due today", "overdue") are UTC.
      timezoneId: "UTC",
      reducedMotion: "reduce",
    });
    const page = await context.newPage();
    page.on("console", (msg) => msg.type() === "error" && console.log(`  [${viewport.name}] console: ${msg.text()}`));
    page.on("pageerror", (err) => console.log(`  [${viewport.name}] pageerror: ${err.message}`));
    const want = (name) => !only || only.has(name);

    if (want("login")) {
      await page.goto(`${baseURL}/login`);
      await settle(page);
      await shot(page, "login", viewport);
      // Validation errors.
      await page.getByRole("button", { name: "Sign in", exact: true }).click();
      await page.waitForTimeout(200);
      await shot(page, "login-errors", viewport);
    }

    await signIn(page, USER);
    const firstOf = async (url) => (await api(page, "GET", url)).data?.[0];
    const deal = await firstOf("/api/v1/deals?filter[stage]=negotiation&per_page=1");
    const company = await firstOf("/api/v1/companies?per_page=1&sort=name");
    const contact = await firstOf("/api/v1/contacts?per_page=1");

    const screens = [
      ["dashboard", "/"],
      ["deals", "/deals"],
      ["deal", deal && `/deals/${deal.id}`],
      ["companies", "/companies"],
      ["company", company && `/companies/${company.id}`],
      ["contacts", "/contacts"],
      ["contact", contact && `/contacts/${contact.id}`],
      ["tasks", "/tasks?scope=everyone&tab=upcoming"],
      ["tasks-empty", "/tasks"],
      ["settings", "/settings"],
      ["users", "/settings/users"],
      ["audit", "/settings/audit"],
      ["empty", "/companies?search=zzzz-nothing-here"],
      ["missing", "/deals/00000000-0000-0000-0000-000000000000"],
    ];
    for (const [name, url] of screens) {
      if (!url || !want(name)) continue;
      await page.goto(`${baseURL}${url}`);
      await settle(page);
      await shot(page, name, viewport, { fullPage: name !== "deals" });
    }

    if (want("loading")) {
      // Hold the dashboard's data back to show the loading state.
      await page.route("**/api/v1/dashboard", async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 4000));
        await route.continue().catch(() => {});
      });
      await page.goto(`${baseURL}/`);
      await page.getByRole("heading", { level: 1, name: "Dashboard" }).waitFor();
      await page.waitForTimeout(500);
      await shot(page, "loading", viewport, { fullPage: false });
      await page.unrouteAll({ behavior: "ignoreErrors" });
    }

    if (want("palette")) {
      await page.goto(`${baseURL}/`);
      await settle(page);
      await page.keyboard.press("Control+k");
      await page.getByRole("dialog").waitFor();
      await page.waitForTimeout(250);
      await shot(page, "palette", viewport, { fullPage: false });
      await page.keyboard.type("north");
      await page.waitForTimeout(900);
      await shot(page, "palette-search", viewport, { fullPage: false });
      await page.keyboard.press("Escape");
    }

    if (want("dialog")) {
      await page.goto(`${baseURL}/deals`);
      await settle(page);
      await page.getByRole("button", { name: "New deal" }).click();
      const dialog = page.getByRole("dialog", { name: "New deal" });
      await dialog.waitFor();
      await dialog.getByRole("button", { name: "Create deal" }).click();
      await page.waitForTimeout(300);
      await shot(page, "dialog", viewport, { fullPage: false });
      await page.keyboard.press("Escape");
    }

    if (want("notifications")) {
      await page.goto(`${baseURL}/`);
      await settle(page);
      await page.getByRole("button", { name: /^Notifications/ }).click();
      await page.waitForTimeout(600);
      await shot(page, "notifications", viewport, { fullPage: false });
      await page.keyboard.press("Escape");
    }

    if (want("menus") && !viewport.isMobile) {
      // The sidebar's app switcher and account menu, open.
      await page.goto(`${baseURL}/`);
      await settle(page);
      await page.getByRole("button", { name: "Navree CRM menu" }).click();
      await page.waitForTimeout(300);
      await shot(page, "switcher", viewport, { fullPage: false });
      await page.keyboard.press("Escape");
      await page.getByRole("button", { name: /^Account menu for/ }).click();
      await page.waitForTimeout(300);
      await shot(page, "user-menu", viewport, { fullPage: false });
      await page.keyboard.press("Escape");
    }

    if (want("sidebar-collapsed") && !viewport.isMobile) {
      // Ctrl B folds the sidebar to its icon rail (kept per device), and back.
      await page.goto(`${baseURL}/deals`);
      await settle(page);
      await page.keyboard.press("Control+b");
      await page.waitForTimeout(400);
      await shot(page, "sidebar-collapsed", viewport, { fullPage: false });
      await page.keyboard.press("Control+b");
      await page.waitForTimeout(300);
    }

    if (want("menu") && viewport.isMobile) {
      await page.goto(`${baseURL}/`);
      await settle(page);
      await page.getByRole("button", { name: "Open navigation" }).click();
      await page.waitForTimeout(400);
      await shot(page, "menu", viewport, { fullPage: false });
      await page.keyboard.press("Escape");
    }

    if (want("toast")) {
      await page.goto(`${baseURL}/tasks?scope=everyone&tab=upcoming`);
      await settle(page);
      const box = page.getByRole("checkbox").first();
      if (await box.count()) {
        await box.click();
        await page.waitForTimeout(700);
        await shot(page, "toast", viewport, { fullPage: false });
      }
    }

    await context.close();
  }
} finally {
  await browser.close();
}
