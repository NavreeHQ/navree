import { expect, test } from "@playwright/test";
import {
  ACCOUNTS,
  BOARD_VIEWPORT,
  callApi,
  dealCard,
  dragToStage,
  signIn,
  stageColumn,
  unique,
  userId,
} from "./support";

type Deal = { id: string; title: string; stage: string; owner_id: string; lost_reason: string | null };

test.use({ viewport: BOARD_VIEWPORT });

test("a rep creates a deal, drags it to proposal, and dropping it on lost asks for a reason", async ({ page }) => {
  await signIn(page, "rep1");
  await page.goto("/deals");
  const title = unique("Robotics fleet pilot");

  // Create the deal.
  await page.getByRole("button", { name: "New deal" }).click();
  const dialog = page.getByRole("dialog", { name: "New deal" });
  await dialog.getByLabel("Deal name").fill(title);
  await dialog.getByRole("combobox", { name: /Company/ }).click();
  await page.getByPlaceholder("Search companies").fill("Doaba");
  await page.getByRole("option", { name: "Doaba Logistics" }).click();
  await dialog.getByLabel("Value").fill("12500");
  const created = page.waitForResponse((r) => r.url().endsWith("/api/v1/deals") && r.request().method() === "POST");
  await dialog.getByRole("button", { name: "Create deal" }).click();
  const deal = (await (await created).json()) as Deal;
  expect(deal).toMatchObject({
    title,
    stage: "lead",
    amount_cents: 1_250_000,
    company: { name: "Doaba Logistics" },
  });
  await expect(dialog).toBeHidden();

  const card = dealCard(page, title);
  await expect(card).toHaveAttribute("data-stage", "lead");
  await expect(stageColumn(page, "lead").getByTestId("deal-card").filter({ hasText: title })).toBeVisible();

  // Drag it to Proposal.
  const proposalCount = stageColumn(page, "proposal").getByTestId("column-count");
  const before = Number((await proposalCount.innerText()).split(" ")[0]);
  const moved = page.waitForResponse((r) => r.url().endsWith(`/api/v1/deals/${deal.id}/move`));
  await dragToStage(page, card, "proposal");
  expect((await moved).status()).toBe(200);
  await expect(card).toHaveAttribute("data-stage", "proposal");
  await expect(stageColumn(page, "proposal").getByTestId("deal-card").filter({ hasText: title })).toBeVisible();
  await expect(proposalCount).toHaveText(`${before + 1} deals`);
  expect((await callApi<Deal>(page, "GET", `/api/v1/deals/${deal.id}`)).body.stage).toBe("proposal");

  // Dropping it on Lost asks why; cancelling keeps the deal where it was.
  await dragToStage(page, card, "lost");
  const lostDialog = page.getByRole("dialog", { name: `Mark “${title}” as lost` });
  await expect(lostDialog).toBeVisible();
  await lostDialog.getByRole("button", { name: "Keep it open" }).click();
  await expect(lostDialog).toBeHidden();
  await expect(stageColumn(page, "proposal").getByTestId("deal-card").filter({ hasText: title })).toBeVisible();
  expect((await callApi<Deal>(page, "GET", `/api/v1/deals/${deal.id}`)).body.stage).toBe("proposal");

  // The API itself refuses `lost` without a reason too.
  const refused = await callApi<{ statusCode: number; errors?: Record<string, string[]> }>(
    page,
    "POST",
    `/api/v1/deals/${deal.id}/move`,
    { stage: "lost" },
  );
  expect(refused.status).toBe(400);
  expect(refused.body.errors?.lost_reason?.length).toBeGreaterThan(0);

  // A reason is required.
  await dragToStage(page, card, "lost");
  await expect(lostDialog).toBeVisible();
  await lostDialog.getByRole("button", { name: "Mark as lost" }).click();
  await expect(lostDialog.getByText("Say why the deal was lost")).toBeVisible();
  await lostDialog.getByLabel("Reason", { exact: true }).fill("Chose an in-house build");
  const lost = page.waitForResponse((r) => r.url().endsWith(`/api/v1/deals/${deal.id}/move`));
  await lostDialog.getByRole("button", { name: "Mark as lost" }).click();
  const lostBody = await lost;
  expect(lostBody.status()).toBe(200);
  expect(JSON.parse(lostBody.request().postData() ?? "{}")).toMatchObject({
    stage: "lost",
    lost_reason: "Chose an in-house build",
  });
  await expect(lostDialog).toBeHidden();
  await expect(card).toHaveAttribute("data-stage", "lost");
  await expect(card).toContainText("Chose an in-house build");
  expect((await callApi<Deal>(page, "GET", `/api/v1/deals/${deal.id}`)).body).toMatchObject({
    stage: "lost",
    lost_reason: "Chose an in-house build",
  });
});

test("a rep cannot edit another rep's deal: no edit control, and the API answers 403", async ({ page }) => {
  await signIn(page, "rep1");
  const rep2 = await userId(page, "rep2");
  const theirs = await callApi<{ data: Deal[] }>(
    page,
    "GET",
    `/api/v1/deals?filter[owner_id]=${rep2}&filter[stage]=qualified`,
  );
  expect(theirs.status).toBe(200);
  const deal = theirs.body.data[0];
  expect(deal, "a qualified deal owned by rep2").toBeTruthy();

  // The deal page has no edit, delete or stage controls for it.
  await page.goto(`/deals/${deal.id}`);
  await expect(page.getByRole("heading", { level: 1, name: deal.title })).toBeVisible();
  await expect(page.getByText(ACCOUNTS.rep2.name).first()).toBeVisible();
  await expect(page.getByRole("button", { name: "Edit" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Delete deal" })).toHaveCount(0);

  // On the board its card is locked (not draggable).
  await page.goto("/deals");
  const card = page.locator(`[data-testid="deal-card"][data-deal-id="${deal.id}"]`);
  await expect(card).toHaveAttribute("aria-roledescription", "deal");
  await expect(card.getByLabel(`Owned by ${ACCOUNTS.rep2.name}; you can't move it`)).toBeVisible();

  // The API refuses the edit and the move (403 from Navree, not the BFF's origin check).
  for (const [method, path, body] of [
    ["PATCH", `/api/v1/deals/${deal.id}`, { title: "Hijacked" }],
    ["POST", `/api/v1/deals/${deal.id}/move`, { stage: "won" }],
  ] as const) {
    const result = await callApi<{ statusCode: number; message: string; error: string }>(page, method, path, body);
    expect(result.status, `${method} ${path}`).toBe(403);
    expect(result.body).toMatchObject({ statusCode: 403, error: "Forbidden" });
    expect(result.body.message).not.toBe("Invalid origin");
  }
  const after = await callApi<Deal>(page, "GET", `/api/v1/deals/${deal.id}`);
  expect(after.body).toMatchObject({ title: deal.title, stage: "qualified", owner_id: rep2 });

  // Control: their own deal does have the edit control.
  const rep1 = await userId(page, "rep1");
  const mine = await callApi<{ data: Deal[] }>(
    page,
    "GET",
    `/api/v1/deals?filter[owner_id]=${rep1}&filter[stage]=qualified`,
  );
  await page.goto(`/deals/${mine.body.data[0].id}`);
  await expect(page.getByRole("heading", { level: 1, name: mine.body.data[0].title })).toBeVisible();
  await expect(page.getByRole("button", { name: "Edit" })).toBeVisible();
});
