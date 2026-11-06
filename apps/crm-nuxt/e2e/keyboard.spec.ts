import { expect, test } from "@playwright/test";
import { BOARD_VIEWPORT, callApi, dealCard, signIn, stageColumn, unique } from "./support";

test.use({ viewport: BOARD_VIEWPORT });

test("a deal moves between stages with the keyboard alone", async ({ page }) => {
  await signIn(page, "rep1");
  const companies = await callApi<{ data: { id: string }[] }>(page, "GET", "/api/v1/companies?search=Doaba");
  const title = unique("Keyboard renewal");
  const created = await callApi<{ id: string }>(page, "POST", "/api/v1/deals", {
    title,
    company_id: companies.body.data[0].id,
    amount_cents: 1_000_000,
  });
  expect(created.status).toBe(201);

  await page.goto("/deals");
  const card = dealCard(page, title);
  await expect(card).toHaveAttribute("data-stage", "lead");
  await expect(card).toHaveAttribute("aria-roledescription", "draggable deal");

  // Space picks it up, two Right arrows carry it to Proposal, Space drops it.
  await card.focus();
  await page.keyboard.press("Space");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowRight");
  await expect(stageColumn(page, "proposal").getByTestId("deal-card").filter({ hasText: title })).toBeVisible();
  const moved = page.waitForResponse((r) => r.url().endsWith(`/api/v1/deals/${created.body.id}/move`));
  await page.keyboard.press("Space");
  expect((await moved).status()).toBe(200);
  await expect(dealCard(page, title)).toHaveAttribute("data-stage", "proposal");
  await expect(dealCard(page, title)).toBeFocused();

  // Escape puts a picked-up card back where it was.
  await page.keyboard.press("Space");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("Escape");
  await expect(stageColumn(page, "proposal").getByTestId("deal-card").filter({ hasText: title })).toBeVisible();
  expect((await callApi<{ stage: string }>(page, "GET", `/api/v1/deals/${created.body.id}`)).body.stage).toBe("proposal");

  // Enter opens the deal.
  await dealCard(page, title).focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { level: 1, name: title })).toBeVisible();
});
