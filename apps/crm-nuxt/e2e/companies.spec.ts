import { expect, test } from "@playwright/test";
import { ACCOUNTS, callApi, signIn, unique } from "./support";


test("a rep creates a company and a contact at it", async ({ page }) => {
  await signIn(page, "rep1");
  const company = unique("Zirakpur Robotics");
  const domain = `${company.toLowerCase().replace(/[^a-z0-9]+/g, "-")}.example`;

  // New company from the list.
  await page.goto("/companies");
  await page.getByRole("button", { name: "New company" }).click();
  const companyDialog = page.getByRole("dialog", { name: "New company" });
  await companyDialog.getByLabel("Name").fill(company);
  await companyDialog.getByLabel("Domain").fill(domain);
  await companyDialog.getByLabel("Industry").fill("Robotics");
  await companyDialog.getByLabel("City").fill("Utrecht");
  const created = page.waitForResponse((r) => r.url().endsWith("/api/v1/companies") && r.request().method() === "POST");
  await companyDialog.getByRole("button", { name: "Create company" }).click();
  expect((await created).status()).toBe(201);
  await expect(companyDialog).toBeHidden();
  await expect(page.getByText(`${company} added`)).toBeVisible();

  // It is listed (search), owned by the rep who created it.
  await page.getByPlaceholder("Search by name or domain").fill(company);
  const row = page.getByRole("row").filter({ hasText: company });
  await expect(row).toHaveCount(1);
  await expect(row).toContainText(domain);
  await expect(row).toContainText(ACCOUNTS.rep1.name);

  // A contact at that company, from the company page.
  await row.getByRole("link", { name: company }).click();
  await expect(page.getByRole("heading", { level: 1, name: company })).toBeVisible();
  await page.getByRole("tab", { name: /Contacts/ }).click();
  await page.getByRole("button", { name: "New contact" }).click();
  const contactDialog = page.getByRole("dialog", { name: "New contact" });
  const lastName = unique("Oberoi").replace(" ", "-");
  await contactDialog.getByLabel("First name").fill("Anu");
  await contactDialog.getByLabel("Last name").fill(lastName);
  await contactDialog.getByLabel("Email").fill(`anu.${lastName.toLowerCase()}@${domain}`);
  await contactDialog.getByLabel("Job title").fill("Head of Operations");
  const contactCreated = page.waitForResponse(
    (r) => r.url().endsWith("/api/v1/contacts") && r.request().method() === "POST",
  );
  await contactDialog.getByRole("button", { name: "Create contact" }).click();
  const contact = await (await contactCreated).json();
  await expect(contactDialog).toBeHidden();
  expect(contact).toMatchObject({
    __type: "Contact",
    full_name: `Anu ${lastName}`,
    status: "lead",
    company: { name: company },
    owner: { name: ACCOUNTS.rep1.name },
  });

  const contactsTab = page.getByRole("tabpanel");
  await expect(contactsTab.getByRole("link", { name: `Anu ${lastName}` })).toBeVisible();
  await expect(page.getByRole("tab", { name: /Contacts/ })).toContainText("1");

  // The contact list shows it with its company.
  await page.goto("/contacts");
  await page.getByPlaceholder("Search by name, email or title").fill(lastName);
  const contactRow = page.getByRole("row").filter({ hasText: `Anu ${lastName}` });
  await expect(contactRow).toHaveCount(1);
  await expect(contactRow).toContainText(company);

  // And the API has both, owned by the rep.
  const companies = await callApi<{ data: { name: string; owner_id: string }[] }>(
    page,
    "GET",
    `/api/v1/companies?search=${encodeURIComponent(company)}`,
  );
  expect(companies.body.data.map((c) => c.name)).toEqual([company]);
  expect(contact.owner_id).toBe(companies.body.data[0].owner_id);
});
