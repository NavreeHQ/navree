import { expect, test } from "@playwright/test";
import { callApi, expectLoginRedirect, signIn } from "./support";

test("mutations from another origin are refused by the BFF (CSRF)", async ({ page, request }) => {
  await signIn(page, "rep1");
  const cookies = await page.context().cookies();
  const cookie = cookies.map((c) => `${c.name}=${c.value}`).join("; ");

  // Same cookies, foreign Origin: 403 before anything reaches Navree.
  const forged = await request.post("/api/v1/companies", {
    headers: { cookie, origin: "https://evil.example", "content-type": "application/json" },
    data: { name: "Forged Ltd" },
  });
  expect(forged.status()).toBe(403);
  expect(await forged.json()).toMatchObject({ statusCode: 403, message: "Invalid origin" });

  // No Origin at all is refused too; reads stay allowed.
  const bare = await request.post("/api/v1/companies", { headers: { cookie }, data: { name: "Forged Ltd" } });
  expect(bare.status()).toBe(403);
  const read = await request.get("/api/v1/companies?per_page=1", { headers: { cookie } });
  expect(read.status()).toBe(200);
});

test("an expired access token is refreshed by the BFF, rotating the refresh cookie", async ({ page, context }) => {
  await signIn(page, "rep1");
  const before = Object.fromEntries((await context.cookies()).map((c) => [c.name, c.value]));
  expect(before.navree_rt).toBeTruthy();

  // The access cookie is gone (as when it expires): the next call refreshes once and succeeds.
  await context.clearCookies({ name: "navree_at" });
  const result = await callApi<{ email: string }>(page, "GET", "/api/auth/me");
  expect(result.status).toBe(200);
  expect(result.body.email).toBe("rep1@navree.test");

  const after = Object.fromEntries((await context.cookies()).map((c) => [c.name, c.value]));
  expect(after.navree_at).toBeTruthy();
  expect(after.navree_rt).toBeTruthy();
  expect(after.navree_rt).not.toBe(before.navree_rt);

  // The app keeps working on the new pair.
  await page.goto("/companies");
  await expect(page.getByRole("heading", { level: 1, name: "Companies" })).toBeVisible();
  await expect(page.getByRole("row").nth(1)).toBeVisible();
});

test("signing out revokes the session and clears the cookies", async ({ page, context }) => {
  await signIn(page, "rep1");
  const refresh = (await context.cookies()).find((c) => c.name === "navree_rt")!.value;

  await page.getByRole("button", { name: /Account menu for/ }).first().click();
  await page.getByRole("menuitem", { name: "Sign out", exact: true }).click();
  await expect(page).toHaveURL(/\/login/);
  expect((await context.cookies()).map((c) => c.name)).not.toContain("navree_rt");

  // The old refresh token no longer works at the API.
  const replay = await page.request.post("http://127.0.0.1:8092/api/auth/refresh", { data: { refresh_token: refresh } });
  expect(replay.status()).toBe(401);
  await page.goto("/deals");
  await expectLoginRedirect(page, "/deals");
});
