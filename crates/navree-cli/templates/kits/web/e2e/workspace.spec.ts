import { expect, test } from "@playwright/test";
import { DEMO, signIn, uniqueEmail } from "./support";

test("the owner invites a member and creates a project", async ({ page }) => {
  await signIn(page, DEMO.owner);
  await expect(page).toHaveURL(new RegExp(`/w/${DEMO.workspace}`));

  await page.getByRole("link", { name: "Members" }).first().click();
  const email = uniqueEmail("max");
  await page.getByLabel("Email").fill(email);
  await page.getByRole("button", { name: "Send invitation" }).click();
  await expect(page.getByText(`Invitation sent to ${email}.`)).toBeVisible();
  await expect(page.getByText(email)).toBeVisible();

  await page.getByRole("link", { name: "Projects" }).first().click();
  await page.getByRole("button", { name: "New project" }).click();
  const name = `Launch ${Date.now()}`;
  await page.getByLabel("Name").fill(name);
  await page.getByRole("button", { name: "Create project" }).click();
  await expect(page.getByRole("link", { name })).toBeVisible();
});

test("the owner upgrades the plan through the fake checkout", async ({ page }) => {
  await signIn(page, DEMO.owner);
  await page.goto(`/w/${DEMO.workspace}/settings/billing`);
  const start = page.getByRole("button", { name: "Start the trial" }).first();
  if (await start.isVisible()) {
    await start.click();
    await expect(page.getByRole("heading", { name: "Checkout" })).toBeVisible();
    await page.getByRole("button", { name: "Pay" }).click();
  }
  await expect(page.getByText(/Trial until|renews/)).toBeVisible();
});

test("viewers and members don't see what they can't do", async ({ page }) => {
  await signIn(page, DEMO.member);
  await expect(page).toHaveURL(new RegExp(`/w/${DEMO.workspace}`));
  await expect(page.getByRole("link", { name: "Billing" })).toHaveCount(0);
  await page.goto(`/w/${DEMO.workspace}/settings/members`);
  await expect(page.getByRole("button", { name: "Send invitation" })).toHaveCount(0);
});
