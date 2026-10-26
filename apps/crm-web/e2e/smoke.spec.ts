import { expect, test } from "@playwright/test";

test("login page renders", async ({ page }) => {
  await page.goto("/login");

  await expect(page).toHaveTitle(/Sign in/);
  await expect(page.getByRole("heading", { level: 1, name: "Sign in" })).toBeVisible();
  await expect(page.getByLabel("Email")).toBeVisible();
  await expect(page.getByLabel("Password")).toBeVisible();
  await expect(page.getByRole("button", { name: "Sign in", exact: true })).toBeEnabled();
});

test("signed-out visitors are sent to the login page", async ({ page }) => {
  await page.goto("/deals");

  await expect(page).toHaveURL(/\/login\?next=%2Fdeals$/);
});

test("a rep signs in with the mock API and lands on the dashboard", async ({ page }) => {
  test.skip(process.env.E2E_REAL_API === "1", "Uses the mock API's demo accounts");
  await page.goto("/login");

  await page.getByLabel("Email").fill("rep1@navis.test");
  await page.getByLabel("Password").fill("password123");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();

  await expect(page).toHaveURL("/");
  await expect(page.getByRole("heading", { level: 1, name: "Dashboard" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Pipeline by stage" })).toBeVisible();
});
