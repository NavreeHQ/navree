import { expect, test } from "@playwright/test";
import { signIn, signOut, totp, uniqueEmail } from "./support";

test("two-step verification: turn it on, then sign in with a code", async ({ page }) => {
  const email = uniqueEmail("grace");
  const password = "a long enough password";
  await page.goto("/sign-up");
  await page.getByLabel("Name").fill("Grace Hopper");
  await page.getByLabel("Email").fill(email);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Create account" }).click();
  await page.getByRole("link", { name: "Continue" }).click();
  await page.getByLabel("Workspace name").fill(`Compilers ${Date.now()}`);
  await page.getByRole("button", { name: "Create workspace" }).click();

  await page.getByRole("button", { name: "Account menu" }).click();
  await page.getByRole("menuitem", { name: "Password and security" }).click();
  await page.getByRole("button", { name: "Turn on" }).click();
  const secret = (await page.locator("code").first().textContent()) ?? "";
  await page.getByLabel("Code from the app").fill(totp(secret));
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByText("Save these recovery codes")).toBeVisible();

  await signOut(page);
  await signIn(page, email, password);
  await expect(page.getByRole("heading", { name: "Two-step verification" })).toBeVisible();
  // A fresh code: the confirmation spent the current one.
  await page.waitForTimeout(31_000);
  await page.getByLabel("Authentication code").fill(totp(secret));
  await page.getByRole("button", { name: "Verify" }).click();
  await expect(page.getByRole("heading", { name: /Welcome, Grace/ })).toBeVisible();
});
