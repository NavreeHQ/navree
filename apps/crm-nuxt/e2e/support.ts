import { expect, test, type Browser, type Locator, type Page } from "@playwright/test";

/** The crm-demo seeder's accounts; every one uses `password123`. */
export const ACCOUNTS = {
  admin: { email: "admin@navree.test", name: "Daljit Kaur" },
  manager: { email: "manager@navree.test", name: "Manpreet Gill" },
  rep1: { email: "rep1@navree.test", name: "Priya Verma" },
  rep2: { email: "rep2@navree.test", name: "Tarun Bhatia" },
} as const;
export type Account = keyof typeof ACCOUNTS;
export const PASSWORD = "password123";

/** A name no other test (or earlier run) has used. */
export function unique(label: string): string {
  return `${label} ${Date.now().toString(36).slice(-5)}${Math.floor(Math.random() * 1296).toString(36)}`;
}

/** Today's date in UTC (`YYYY-MM-DD`), the API's day boundary. */
export function todayUtc(): string {
  return new Date().toISOString().slice(0, 10);
}

/** Signs in through the login page and waits for the dashboard. */
export async function signIn(page: Page, who: Account): Promise<void> {
  await page.goto("/login");
  await page.getByLabel("Email").fill(ACCOUNTS[who].email);
  await page.getByLabel("Password").fill(PASSWORD);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page).toHaveURL("/");
  await expect(page.getByRole("heading", { level: 1, name: "Dashboard" })).toBeVisible();
}

/**
 * Waits for the login page with `next` pointing back at `target`. The server's redirect encodes
 * `next` fully (`%2Fdeals`); once the app boots, Nuxt's router rewrites the address with its own,
 * lighter query encoding (`/deals`). Both mean the same, so compare the decoded value.
 */
export async function expectLoginRedirect(page: Page, target: string): Promise<void> {
  await expect
    .poll(() => {
      const url = new URL(page.url());
      return url.pathname === "/login" ? url.searchParams.get("next") : null;
    })
    .toBe(target);
}

export type ApiResult<T = unknown> = { status: number; body: T };

/**
 * Calls the API from the page the way the app does: same-origin through the BFF,
 * with the browser's cookies and Origin header (no tokens in the page).
 */
export async function callApi<T = unknown>(
  page: Page,
  method: string,
  path: string,
  body?: unknown,
): Promise<ApiResult<T>> {
  return page.evaluate(
    async ({ method, path, body }) => {
      const response = await fetch(path, {
        method,
        headers: { Accept: "application/json", ...(body === undefined ? {} : { "Content-Type": "application/json" }) },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      const text = await response.text();
      let parsed: unknown = text;
      try {
        parsed = text ? JSON.parse(text) : null;
      } catch {
        // not JSON
      }
      return { status: response.status, body: parsed as T };
    },
    { method, path, body },
  );
}

/** The id of a demo account, looked up through the API. */
export async function userId(page: Page, who: Account): Promise<string> {
  const { status, body } = await callApi<{ data: { id: string; email: string }[] }>(
    page,
    "GET",
    `/api/v1/users?search=${encodeURIComponent(ACCOUNTS[who].email)}`,
  );
  expect(status).toBe(200);
  const user = body.data.find((u) => u.email === ACCOUNTS[who].email);
  expect(user, `user ${ACCOUNTS[who].email}`).toBeTruthy();
  return user!.id;
}

/** A deal card on the board, by title. */
export function dealCard(page: Page, title: string): Locator {
  return page.getByTestId("deal-card").filter({ hasText: title });
}

/** A board column (`section[data-stage]`). */
export function stageColumn(page: Page, stage: string): Locator {
  return page.locator(`section[data-stage="${stage}"]`);
}

/**
 * Drags a deal card onto a stage column with real mouse events: the board starts a
 * drag only after the pointer has moved 6 px, and finds the column and the slot
 * under the pointer on every move.
 */
export async function dragToStage(page: Page, card: Locator, stage: string): Promise<void> {
  const column = stageColumn(page, stage);
  await card.scrollIntoViewIfNeeded();
  const from = await card.boundingBox();
  const to = await column.boundingBox();
  if (!from || !to) throw new Error(`card or column "${stage}" is not visible`);
  // Grab the card by its bottom-left corner (the amount), not by the title link.
  const startX = from.x + 16;
  const startY = from.y + from.height - 12;
  await page.mouse.move(startX, startY);
  await page.mouse.down();
  await page.mouse.move(startX + 15, startY + 15, { steps: 5 });
  const targetX = to.x + to.width / 2;
  const targetY = to.y + Math.min(to.height / 2, 140);
  await page.mouse.move(targetX, targetY, { steps: 20 });
  await page.mouse.move(targetX, targetY + 5, { steps: 3 });
  await expect(page.getByTestId("deal-drag-overlay"), "the drag has started").toBeVisible();
  // Hover over the column for a moment, as a person would, before dropping.
  await page.waitForTimeout(150);
  await page.mouse.up();
  // The overlay goes away when the card has been dropped.
  await expect(page.getByTestId("deal-drag-overlay")).toHaveCount(0);
}

/** Wide enough for all six board columns side by side. */
export const BOARD_VIEWPORT = { width: 2200, height: 1100 };

/**
 * A page in a fresh browser context with the project's options (base URL, time zone),
 * e.g. a second person in a multi-user test. Close it with `page.context().close()`.
 */
export async function newPersonPage(
  browser: Browser,
  options: { viewport?: { width: number; height: number } } = {},
): Promise<Page> {
  const { baseURL, timezoneId } = test.info().project.use;
  const context = await browser.newContext({ baseURL, timezoneId, ...options });
  return context.newPage();
}
