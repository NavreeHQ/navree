// Screenshots of every screen at 1440 and 390 px, for design review.
//
//   node scripts/screenshots.mjs [baseURL] [outDir]
//
// Defaults: http://localhost:3000 and ./screenshots. Needs a running app (pnpm dev
// against a seeded crm-api, or pnpm dev:mock) and signs in as the crm-demo accounts.
// SHOTS=login,dashboard limits the run to some screens.
import { mkdirSync } from "node:fs";
import path from "node:path";
import { chromium } from "@playwright/test";

const baseURL = process.argv[2] ?? "http://localhost:3000";
const outDir = path.resolve(process.argv[3] ?? "screenshots");
const only = process.env.SHOTS ? new Set(process.env.SHOTS.split(",")) : null;
const PASSWORD = "password123";

const VIEWPORTS = [
  { name: "1440", width: 1440, height: 900 },
  { name: "390", width: 390, height: 844, isMobile: true, hasTouch: true, deviceScaleFactor: 2 },
];

mkdirSync(outDir, { recursive: true });

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
  // The Next.js dev-tools badge is not part of the app.
  await page.addStyleTag({ content: "nextjs-portal{display:none!important}" }).catch(() => {});
  await page.waitForLoadState("networkidle").catch(() => {});
  await page
    .waitForFunction(() => !document.querySelector('[aria-busy="true"]'), null, { timeout: 15_000 })
    .catch(() => {});
  await page.waitForTimeout(400);
}

async function shot(page, name, viewport, { fullPage = true } = {}) {
  const file = path.join(outDir, `${name}-${viewport.name}.png`);
  const height = fullPage ? await page.evaluate(() => document.documentElement.scrollHeight) : 0;
  if (height > viewport.height) {
    // Grow the window to the whole page instead of stitching a full-page capture, so the
    // fixed, full-height sidebar runs down the whole image as it does on screen.
    await page.setViewportSize({ width: viewport.width, height });
    await page.waitForTimeout(150);
    await page.screenshot({ path: file });
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
  } else {
    await page.screenshot({ path: file });
  }
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
      timezoneId: "UTC",
      reducedMotion: "reduce",
    });
    const page = await context.newPage();
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

    await signIn(page, "admin@navree.test");
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
      await page.addStyleTag({ content: "nextjs-portal{display:none!important}" }).catch(() => {});
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

    if (want("menu") && viewport.isMobile) {
      await page.goto(`${baseURL}/`);
      await settle(page);
      await page.getByRole("button", { name: "Toggle Sidebar" }).click();
      await page.waitForTimeout(400);
      await shot(page, "menu", viewport, { fullPage: false });
    }

    if (want("shell") && !viewport.isMobile) {
      // The sidebar-07 shell: the workspace and account menus, notifications, collapsed to icons.
      await page.goto(`${baseURL}/companies`);
      await settle(page);
      for (const [name, button] of [
        ["workspace-menu", { name: "Workspace menu" }],
        ["account-menu", { name: /Account menu/ }],
        ["notifications", { name: /Notifications/ }],
      ]) {
        await page.getByRole("button", button).click();
        await page.waitForTimeout(300);
        await shot(page, name, viewport, { fullPage: false });
        await page.keyboard.press("Escape");
      }
      // Escape returns focus to the last trigger; park it on the content so no focus ring shows.
      await page.evaluate(() => {
        document.getElementById("content")?.focus({ preventScroll: true });
        window.scrollTo(0, 0);
      });
      await page.keyboard.press("Control+b");
      await page.waitForTimeout(400);
      await shot(page, "collapsed", viewport, { fullPage: false });
      await page.keyboard.press("Control+b");
      await page.waitForTimeout(400);
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
