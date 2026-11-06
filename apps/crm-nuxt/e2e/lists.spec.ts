import { expect, test } from "@playwright/test";
import { signIn } from "./support";

test("the contacts list sends its status filter as filter[status], and the API applies it", async ({ page }) => {
  await signIn(page, "manager");
  await page.goto("/contacts");

  const filtered = page.waitForRequest((r) => r.url().includes("/api/v1/contacts") && r.url().includes("status"));
  await page.getByRole("combobox", { name: "Filter by status" }).click();
  await page.getByRole("option", { name: "Customer" }).click();

  const request = await filtered;
  const sent = new URL(request.url()).searchParams;
  expect(sent.get("filter[status]")).toBe("customer");
  expect(sent.has("status")).toBe(false);

  const response = await request.response();
  expect(response?.status()).toBe(200);
  const body = (await response?.json()) as { data: { status: string }[] };
  for (const contact of body.data) expect(contact.status).toBe("customer");
});
