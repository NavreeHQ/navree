import { expect, test } from "@playwright/test";
import { newPersonPage, PASSWORD, requireRealApi, signIn, unique } from "./support";

test.beforeEach(() => requireRealApi());

test("an admin adds a user with a role, and the audit log shows it", async ({ page, browser }) => {
  await signIn(page, "admin");
  await page.goto("/settings/users");
  await expect(page.getByTestId("user-row").first()).toBeVisible();
  const existing = await page.getByTestId("user-row").count();
  expect(existing).toBeGreaterThanOrEqual(4);

  const name = unique("Ritu Nanda");
  const email = `${name.toLowerCase().replace(/[^a-z0-9]+/g, ".")}@navree.test`;
  await page.getByRole("button", { name: "Add user" }).click();
  const dialog = page.getByRole("dialog", { name: "Add a user" });
  await dialog.getByLabel("Name").fill(name);
  await dialog.getByLabel("Email").fill(email);
  await dialog.getByLabel("Temporary password").fill(PASSWORD);
  await dialog.getByRole("combobox", { name: "Role" }).click();
  await page.getByRole("option", { name: "Manager" }).click();
  const created = page.waitForResponse((r) => r.url().endsWith("/api/v1/users") && r.request().method() === "POST");
  await dialog.getByRole("button", { name: "Add user" }).click();
  const user = await (await created).json();
  expect(user).toMatchObject({ __type: "User", name, email, roles: ["manager"] });
  await expect(dialog).toBeHidden();
  await expect(page.getByText(`${name} added as manager`)).toBeVisible();

  const row = page.getByTestId("user-row").filter({ hasText: email });
  await expect(row).toBeVisible();
  await expect(row.getByRole("combobox", { name: `Role for ${name}` })).toHaveText("Manager");
  await expect(page.getByTestId("user-row")).toHaveCount(existing + 1);

  // The audit log records the new account and its role, newest first.
  await page.goto("/settings/audit");
  const rows = page.getByTestId("audit-row");
  await expect(rows.first()).toBeVisible();
  expect(await rows.count()).toBeGreaterThan(1);
  await expect(rows.filter({ hasText: "User created" }).filter({ hasText: email }).first()).toBeVisible();
  await expect(rows.filter({ hasText: "Role assigned" }).first()).toBeVisible();

  // Filtering by action narrows the log to that action.
  await page.getByRole("combobox", { name: "Filter by action" }).click();
  await page.getByRole("option", { name: "User created" }).click();
  await expect(page).toHaveURL(/action=user\.created/);
  await expect(rows.first()).toContainText("User created");
  for (const text of await rows.allInnerTexts()) expect(text).toContain("User created");

  // The new manager can sign in straight away.
  const other = await newPersonPage(browser);
  await other.goto("/login");
  await other.getByLabel("Email").fill(email);
  await other.getByLabel("Password").fill(PASSWORD);
  await other.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(other.getByRole("heading", { level: 1, name: "Dashboard" })).toBeVisible();
  await expect(other.getByText(name).first()).toBeVisible();
  await other.context().close();
});
