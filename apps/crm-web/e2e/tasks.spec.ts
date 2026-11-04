import { expect, test } from "@playwright/test";
import { ACCOUNTS, callApi, requireRealApi, signIn, todayUtc, unique } from "./support";

type Activity = { id: string; subject: string; kind: string; due_at: string; completed_at: string | null };

test.beforeEach(() => requireRealApi());

test("a rep adds a task due today, finds it under Today and completes it", async ({ page }) => {
  await signIn(page, "rep1");
  await page.goto("/tasks");
  await expect(page.getByRole("tab", { name: /Today/, selected: true })).toBeVisible();
  const subject = unique("Send the revised proposal");

  await page.getByRole("button", { name: "New task" }).click();
  const dialog = page.getByRole("dialog", { name: "New task" });
  await dialog.getByLabel("Task").fill(subject);
  await dialog.getByRole("combobox", { name: /Related to/ }).click();
  await page.getByPlaceholder("Search deals").fill("Fleet telematics");
  await page.getByRole("option", { name: "Fleet telematics rollout" }).click();
  // Later today (UTC): the API's "today"; the browser runs in UTC.
  await dialog.getByLabel("Due").fill(`${todayUtc()}T23:30`);
  await expect(dialog.getByLabel("Assignee")).toContainText(ACCOUNTS.rep1.name);
  const created = page.waitForResponse(
    (r) => r.url().endsWith("/api/v1/activities") && r.request().method() === "POST",
  );
  await dialog.getByRole("button", { name: "Add task" }).click();
  const task = (await (await created).json()) as Activity;
  expect(task).toMatchObject({ kind: "task", subject, subject_type: "deal", completed_at: null });
  expect(task.due_at.slice(0, 10)).toBe(todayUtc());
  await expect(dialog).toBeHidden();

  // It is listed under Today.
  const todayList = page.getByRole("region", { name: "Today tasks" });
  const row = todayList.getByTestId("task-row").filter({ hasText: subject });
  await expect(row).toBeVisible();
  await expect(row).toContainText("Fleet telematics rollout");
  const due = await callApi<{ data: Activity[] }>(
    page,
    "GET",
    "/api/v1/activities?filter[due]=today&filter[kind]=task&per_page=100",
  );
  expect(due.body.data.map((a) => a.id)).toContain(task.id);

  // Complete it: it leaves Today and shows under Completed.
  const completed = page.waitForResponse((r) => r.url().endsWith(`/api/v1/activities/${task.id}/complete`));
  await row.getByRole("checkbox", { name: `Complete ${subject}` }).click();
  expect((await completed).status()).toBe(200);
  await expect(page.getByText(`Completed “${subject}”`)).toBeVisible();
  await expect(todayList.getByTestId("task-row").filter({ hasText: subject })).toHaveCount(0);

  await page.getByRole("tab", { name: /Completed/ }).click();
  const done = page
    .getByRole("region", { name: "Completed tasks" })
    .getByTestId("task-row")
    .filter({ hasText: subject });
  await expect(done).toBeVisible();
  await expect(done.getByRole("checkbox", { name: `${subject} is complete` })).toBeChecked();

  const stored = await callApi<{ data: Activity[] }>(
    page,
    "GET",
    "/api/v1/activities?filter[completed]=true&filter[kind]=task&per_page=100",
  );
  expect(stored.body.data.find((a) => a.id === task.id)?.completed_at).toBeTruthy();
});
