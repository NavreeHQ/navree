import { expect, test } from "@playwright/test";
import { ACCOUNTS, PASSWORD } from "./support";


test("a wrong password is refused with a clear message and no session", async ({ page, context }) => {
  await page.goto("/login");
  await page.getByLabel("Email").fill(ACCOUNTS.rep1.email);
  await page.getByLabel("Password").fill("not-the-password");
  const login = page.waitForResponse((r) => r.url().endsWith("/api/auth/login"));
  await page.getByRole("button", { name: "Sign in", exact: true }).click();

  expect((await login).status()).toBe(401);
  await expect(
    page.getByRole("alert").filter({ hasText: "That email and password don't match an account." }),
  ).toBeVisible();
  await expect(page).toHaveURL(/\/login$/);
  await expect(page.getByLabel("Password")).toHaveValue("");
  const cookies = await context.cookies();
  expect(cookies.map((c) => c.name)).not.toContain("navree_at");
});

test("signing in lands on the dashboard with the tokens in httpOnly cookies only", async ({ page, context }) => {
  await page.goto("/login");
  await page.getByLabel("Email").fill(ACCOUNTS.rep1.email);
  await page.getByLabel("Password").fill(PASSWORD);
  const login = page.waitForResponse((r) => r.url().endsWith("/api/auth/login"));
  await page.getByRole("button", { name: "Sign in", exact: true }).click();

  // The BFF answers with the user only; the tokens stay on the server side of the cookie hop.
  const body = await (await login).json();
  expect(Object.keys(body)).toEqual(["user"]);
  expect(body.user).toMatchObject({ email: ACCOUNTS.rep1.email, name: ACCOUNTS.rep1.name, roles: ["rep"] });

  await expect(page).toHaveURL("/");
  await expect(page.getByRole("heading", { level: 1, name: "Dashboard" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Pipeline by stage" })).toBeVisible();
  await expect(page.getByText(ACCOUNTS.rep1.name).first()).toBeVisible();

  const cookies = Object.fromEntries((await context.cookies()).map((c) => [c.name, c]));
  for (const name of ["navree_at", "navree_rt"]) {
    expect(cookies[name], name).toBeTruthy();
    expect(cookies[name].httpOnly, `${name} is httpOnly`).toBe(true);
    expect(cookies[name].sameSite).toBe("Lax");
  }
  expect(await page.evaluate(() => document.cookie)).not.toContain("navree_");

  // A signed-in visitor going back to /login is sent on to the app.
  await page.goto("/login");
  await expect(page).toHaveURL("/");
});
