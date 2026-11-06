import { expect, test, type Page } from "@playwright/test";
import {
  BOARD_VIEWPORT,
  callApi,
  dealCard,
  dragToStage,
  newPersonPage,
  requireRealApi,
  signIn,
  stageColumn,
  unique,
} from "./support";

test.use({ viewport: BOARD_VIEWPORT });
test.beforeEach(() => requireRealApi());

/** Records the Pusher-protocol frames a page receives on its real-time socket. */
function recordSocketFrames(page: Page): string[] {
  const frames: string[] = [];
  page.on("websocket", (socket) => {
    socket.on("framereceived", ({ payload }) =>
      frames.push(typeof payload === "string" ? payload : payload.toString()),
    );
  });
  return frames;
}

function subscribed(frames: string[], channel: string): boolean {
  return frames.some((f) => f.includes("pusher_internal:subscription_succeeded") && f.includes(`"${channel}"`));
}

test("a deal moved in one browser moves on another person's board without a reload", async ({ page, browser }) => {
  // The watcher: a manager with the board open in another browser context.
  const watcher = await newPersonPage(browser, { viewport: BOARD_VIEWPORT });
  const frames = recordSocketFrames(watcher);
  await signIn(watcher, "manager");
  await watcher.goto("/deals");
  await expect(watcher.getByTestId("deal-card").first()).toBeVisible();
  // The socket is up and private-crm.deals is authorised (POST /broadcasting/auth through the BFF).
  await expect.poll(() => subscribed(frames, "private-crm.deals"), { timeout: 15_000 }).toBe(true);
  await watcher.evaluate(() => Object.assign(window, { __sameDocument: true }));

  // The mover: a rep who creates a deal (through the API) and opens their board.
  await signIn(page, "rep1");
  const companies = await callApi<{ data: { id: string }[] }>(page, "GET", "/api/v1/companies?search=Doaba");
  const title = unique("Realtime rollout");
  const created = await callApi<{ id: string }>(page, "POST", "/api/v1/deals", {
    title,
    company_id: companies.body.data[0].id,
    amount_cents: 2_000_000,
  });
  expect(created.status).toBe(201);
  const dealId = created.body.id;

  // Creating it already reaches the watcher's board.
  const watched = dealCard(watcher, title);
  await expect(watched).toHaveAttribute("data-stage", "lead");

  // Drag it to Negotiation in the mover's browser.
  await page.goto("/deals");
  const card = dealCard(page, title);
  await expect(card).toHaveAttribute("data-stage", "lead");
  const moved = page.waitForResponse((r) => r.url().endsWith(`/api/v1/deals/${dealId}/move`));
  await dragToStage(page, card, "negotiation");
  expect((await moved).status()).toBe(200);
  await expect(card).toHaveAttribute("data-stage", "negotiation");

  // The watcher sees it move: pushed as `deal.updated` on private-crm.deals, same document, no reload.
  await expect(watched).toHaveAttribute("data-stage", "negotiation");
  await expect(stageColumn(watcher, "negotiation").getByTestId("deal-card").filter({ hasText: title })).toBeVisible();
  expect(await watcher.evaluate(() => (window as unknown as { __sameDocument?: boolean }).__sameDocument)).toBe(true);
  const pushes = frames.filter((f) => f.includes('"deal.updated"') && f.includes(dealId));
  expect(pushes.length).toBeGreaterThanOrEqual(2);
  expect(pushes.at(-1)).toContain("negotiation");

  await watcher.context().close();
});
