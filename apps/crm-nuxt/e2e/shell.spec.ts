import { expect, test } from "@playwright/test";
import { callApi, signIn } from "./support";

test("the breadcrumb names the screen and the record, and links back to the list", async ({ page }) => {
  await signIn(page, "manager");
  const breadcrumb = page.getByRole("navigation", { name: "Breadcrumb" });
  await expect(breadcrumb).toHaveText("Dashboard");

  const deals = await callApi<{ data: { id: string; title: string }[] }>(page, "GET", "/api/v1/deals?per_page=1");
  const deal = deals.body.data[0];
  await page.goto(`/deals/${deal.id}`);
  await expect(page.getByRole("heading", { level: 1, name: deal.title })).toBeVisible();
  await expect(breadcrumb.locator('[aria-current="page"]')).toHaveText(deal.title);

  await breadcrumb.getByRole("link", { name: "Deals" }).click();
  await expect(page).toHaveURL("/deals");
  await expect(breadcrumb).toHaveText("Deals");
});

test("Ctrl B folds the sidebar to icons, and the choice survives a reload", async ({ page }) => {
  await signIn(page, "rep1");
  const sidebar = page.getByTestId("sidebar");
  await expect(sidebar).toHaveAttribute("data-state", "expanded");
  const nav = page.getByRole("navigation", { name: "Main" });
  await expect(nav.getByText("Sales", { exact: true })).toBeVisible();

  await page.keyboard.press("Control+b");
  await expect(sidebar).toHaveAttribute("data-state", "collapsed");
  await expect(nav.getByText("Sales", { exact: true })).toHaveCount(0);
  // Collapsed, every item keeps its name for assistive tech and still navigates.
  await nav.getByRole("link", { name: "Companies" }).click();
  await expect(page).toHaveURL("/companies");

  await page.reload();
  await expect(sidebar).toHaveAttribute("data-state", "collapsed");
  await page.getByRole("button", { name: "Expand sidebar" }).click();
  await expect(sidebar).toHaveAttribute("data-state", "expanded");
});

test("the top bar's Create menu and the sidebar's app menu open where they are and act", async ({ page }) => {
  await signIn(page, "rep1");

  await page.getByRole("button", { name: "Create", exact: true }).click();
  const create = page.getByRole("menu");
  await expect(create).toBeInViewport();
  await create.getByRole("menuitem", { name: /Company/ }).click();
  await expect(page.getByRole("dialog", { name: "New company" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog", { name: "New company" })).toBeHidden();

  await page.getByRole("button", { name: "Navree CRM menu" }).click();
  const app = page.getByRole("menu");
  await expect(app).toBeInViewport();
  await app.getByRole("menuitem", { name: /Keyboard shortcuts/ }).click();
  await expect(page.getByRole("dialog", { name: "Keyboard shortcuts" })).toBeVisible();
});

test.describe("on a phone", () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true });

  test("the navigation opens as a sheet and closes after choosing a screen", async ({ page }) => {
    await signIn(page, "rep1");
    await expect(page.getByRole("complementary", { name: "Sidebar" })).toBeHidden();

    await page.getByRole("button", { name: "Open navigation" }).click();
    const sheet = page.getByRole("dialog", { name: "Navigation" });
    await expect(sheet).toBeVisible();
    await sheet.getByRole("link", { name: "Tasks" }).click();
    await expect(sheet).toBeHidden();
    await expect(page).toHaveURL("/tasks");
    await expect(page.getByRole("heading", { level: 1, name: "Tasks" })).toBeVisible();
  });
});
