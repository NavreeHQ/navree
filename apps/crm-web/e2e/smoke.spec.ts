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

  await page.getByLabel("Email").fill("rep1@navree.test");
  await page.getByLabel("Password").fill("password123");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();

  await expect(page).toHaveURL("/");
  await expect(page.getByRole("heading", { level: 1, name: "Dashboard" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Pipeline by stage" })).toBeVisible();
});

test("the command menu opens with Ctrl+K, jumps to a screen and starts a new deal", async ({ page }) => {
  await page.goto("/login");
  await page.getByLabel("Email").fill("rep1@navree.test");
  await page.getByLabel("Password").fill("password123");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Dashboard" })).toBeVisible();

  // Jump to a screen.
  await page.keyboard.press("Control+k");
  const menu = page.getByRole("dialog", { name: "Command menu" });
  await expect(menu).toBeVisible();
  const searched = page.waitForResponse((r) => r.url().includes("/companies?search=comp"));
  await menu.getByRole("combobox").fill("comp");
  await searched;
  await menu.getByRole("option", { name: "Companies", exact: true }).click();
  await expect(page).toHaveURL("/companies");
  await expect(page.getByRole("heading", { level: 1, name: "Companies" })).toBeVisible();

  // Create: goes to the board and opens its "New deal" dialog.
  await page.keyboard.press("Control+k");
  const creating = page.waitForResponse((r) => r.url().includes("/deals?search=new"));
  await menu.getByRole("combobox").fill("new deal");
  await creating;
  await expect(menu.getByRole("option", { name: "New deal" })).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL("/deals");
  await expect(page.getByRole("dialog", { name: "New deal" })).toBeVisible();
});
