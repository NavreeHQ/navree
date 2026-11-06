import { expect, test } from "@playwright/test";
import { expectLoginRedirect, signIn } from "./support";

test("the login page renders", async ({ page }) => {
  await page.goto("/login");

  await expect(page).toHaveTitle(/Sign in/);
  await expect(page.getByRole("heading", { level: 1, name: "Sign in" })).toBeVisible();
  await expect(page.getByLabel("Email")).toBeVisible();
  await expect(page.getByLabel("Password")).toBeVisible();
  await expect(page.getByRole("button", { name: "Sign in", exact: true })).toBeEnabled();
});

test("signed-out visitors are sent to the login page, and back after signing in", async ({ page }) => {
  // The server answers first: no session cookie, so no app shell, just a redirect.
  const gate = await page.request.get("/deals?owner=nobody", { headers: { accept: "text/html" }, maxRedirects: 0 });
  expect(gate.status()).toBe(302);
  expect(gate.headers().location).toBe("/login?next=%2Fdeals%3Fowner%3Dnobody");

  await page.goto("/deals?owner=nobody");
  await expectLoginRedirect(page, "/deals?owner=nobody");

  await page.getByLabel("Email").fill("rep1@navree.test");
  await page.getByLabel("Password").fill("password123");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page).toHaveURL("/deals?owner=nobody");
  await expect(page.getByRole("heading", { level: 1, name: "Deals" })).toBeVisible();
});

test("every screen renders for a manager, with the sidebar and the palette", async ({ page }) => {
  await signIn(page, "manager");
  for (const [path, heading] of [
    ["/deals", "Deals"],
    ["/companies", "Companies"],
    ["/contacts", "Contacts"],
    ["/tasks", "Tasks"],
    ["/settings", "Settings"],
  ] as const) {
    await page.getByRole("navigation", { name: "Main" }).getByRole("link", { name: heading }).click();
    await expect(page).toHaveURL(path);
    await expect(page.getByRole("heading", { level: 1, name: heading })).toBeVisible();
  }

  // Ctrl K searches the API and opens a record.
  await page.keyboard.press("Control+k");
  const palette = page.getByRole("dialog", { name: "Search Navree CRM" });
  await expect(palette).toBeVisible();
  await page.keyboard.type("Doaba");
  await expect(palette.getByRole("option", { name: /Doaba Logistics/ }).first()).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(palette).toBeHidden();
  await expect(page.getByRole("heading", { level: 1, name: "Doaba Logistics" })).toBeVisible();

  // `g d` goes to the board.
  await page.keyboard.press("g");
  await page.keyboard.press("d");
  await expect(page).toHaveURL("/deals");
});
