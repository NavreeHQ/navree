import { expect, test } from "@playwright/test";
import { uniqueEmail } from "./support";

test("people sign up and create their first workspace", async ({ page }) => {
  await page.goto("/sign-up");
  await page.getByLabel("Name").fill("Anu Lakhotia");
  await page.getByLabel("Email").fill(uniqueEmail("anu"));
  await page.getByLabel("Password").fill("a long enough password");
  await page.getByRole("button", { name: "Create account" }).click();

  // The verification mail is on its way; continue meanwhile.
  await expect(page.getByRole("heading", { name: "Check your inbox" })).toBeVisible();
  await page.getByRole("link", { name: "Continue" }).click();

  await expect(page.getByRole("heading", { name: "Create a workspace" })).toBeVisible();
  await page.getByLabel("Workspace name").fill("Analytical Engines");
  await page.getByRole("button", { name: "Create workspace" }).click();

  await expect(page).toHaveURL(/\/w\/analytical-engines/);
  await expect(page.getByRole("heading", { name: /Welcome, Anu/ })).toBeVisible();
});
