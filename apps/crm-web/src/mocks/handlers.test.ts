import { getResponse } from "msw";
import { beforeEach, describe, expect, it } from "vitest";
import { AUDIT_ACTIONS, entryLabel } from "@/features/audit/actions";
import { db, resetDatabase, setSessionUser } from "./db";
import { handlers } from "./handlers";

/** One request through the MSW handlers, as the app would send it. */
async function call(method: string, path: string, body?: unknown): Promise<Response> {
  const request = new Request(new URL(path, window.location.origin), {
    method,
    headers: body === undefined ? {} : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const response = await getResponse(handlers, request);
  if (!response) throw new Error(`no mock handler for ${method} ${path}`);
  return response;
}

const offered = new Set<string>(AUDIT_ACTIONS);

describe("mock API audit log mirrors crm-api's actions", () => {
  beforeEach(() => {
    resetDatabase();
    setSessionUser(db.users.find((u) => u.role === "admin")!.id);
  });

  it("records every seeded action under an action the filter offers", () => {
    expect(db.audit.map((e) => e.action).filter((action) => !offered.has(action))).toEqual([]);
  });

  it("records a company or contact deletion as an update of deleted_at, shown as a deletion", async () => {
    // Records the seed has no audit rows about, so the deletion is the only one.
    const untouched = <T extends { id: string }>(rows: T[]) =>
      rows.find((r) => !db.audit.some((e) => e.subject_id === r.id))!;
    for (const [path, type] of [
      [`/api/v1/companies/${untouched(db.companies).id}`, "Company"],
      [`/api/v1/contacts/${untouched(db.contacts).id}`, "Contact"],
    ] as const) {
      expect((await call("DELETE", path)).status).toBe(204);
      const [entry] = db.audit;
      expect(entry).toMatchObject({ action: `${type.toLowerCase()}.updated`, subject_type: type });
      expect(entry.old_values).toEqual({ deleted_at: null });
      expect(entry.new_values?.deleted_at).toEqual(expect.any(String));
      expect(entryLabel(entry)).toBe(`${type} deleted`);

      const found = await call("GET", `/api/audit?action=${entry.action}&subject_id=${entry.subject_id}`);
      expect((await found.json()).meta.total).toBe(1);
    }
  });

  it("records a role change as access.user_roles_synced and a rename as user.updated", async () => {
    const rep = db.users.find((u) => u.role === "rep")!;
    expect((await call("PATCH", `/api/v1/users/${rep.id}`, { name: "Renamed Rep", role: "manager" })).status).toBe(200);

    const [roles, rename] = db.audit;
    expect(roles).toMatchObject({
      kind: "access",
      action: "access.user_roles_synced",
      subject_type: "user",
      subject_id: rep.id,
      old_values: { roles: ["rep"] },
      new_values: { roles: ["manager"] },
    });
    expect(rename).toMatchObject({ kind: "data", action: "user.updated", new_values: { name: "Renamed Rep" } });
    expect([roles.action, rename.action].filter((action) => !offered.has(action))).toEqual([]);
  });

  it("records a new user's role as access.role_assigned", async () => {
    const response = await call("POST", "/api/v1/users", {
      name: "Riley Novak",
      email: "riley.novak@navree.test",
      password: "password123",
      role: "manager",
    });
    expect(response.status).toBe(201);
    const { id } = await response.json();
    expect(db.audit.slice(0, 2).map((e) => [e.action, e.subject_id])).toEqual([
      ["access.role_assigned", id],
      ["user.created", id],
    ]);
  });
});

/** A list endpoint's answer. */
async function getPage<T>(path: string) {
  const response = await call("GET", path);
  const body = (await response.json()) as { data: T[]; meta: { total: number }; errors?: Record<string, string[]> };
  return { status: response.status, body };
}

// MOD-013
describe("mock API lists read the one list grammar", () => {
  beforeEach(() => {
    resetDatabase();
    setSessionUser(db.users.find((u) => u.role === "admin")!.id);
  });

  it("filters deals with filter[stage] and ignores a flat stage", async () => {
    const live = db.deals.filter((d) => !d.deleted_at);
    const qualified = live.filter((d) => d.stage === "qualified");
    expect(qualified.length).toBeGreaterThan(0);
    expect(qualified.length).toBeLessThan(live.length);

    const grammar = await getPage<{ stage: string }>("/api/v1/deals?filter[stage]=qualified&per_page=100");
    expect(grammar.status).toBe(200);
    expect(grammar.body.meta.total).toBe(qualified.length);
    expect(grammar.body.data.every((d) => d.stage === "qualified")).toBe(true);

    const flat = await getPage("/api/v1/deals?stage=qualified&per_page=100");
    expect(flat.status).toBe(200);
    expect(flat.body.meta.total).toBe(live.length);
  });

  it("sorts deals with sort=-amount_cents", async () => {
    const { body } = await getPage<{ amount_cents: number }>("/api/v1/deals?sort=-amount_cents&per_page=100");
    const amounts = body.data.map((d) => d.amount_cents);
    expect(amounts.length).toBeGreaterThan(1);
    expect(amounts).toEqual([...amounts].sort((a, b) => b - a));
  });

  it("filters companies by filter[owner_id] and sorts them with sort=-name", async () => {
    const live = db.companies.filter((c) => !c.deleted_at);
    const ownerId = live[0]!.owner_id;
    const names = live
      .filter((c) => c.owner_id === ownerId)
      .map((c) => c.name)
      .sort((a, b) => b.localeCompare(a));
    expect(names.length).toBeLessThan(live.length);

    const { status, body } = await getPage<{ name: string }>(
      `/api/v1/companies?filter[owner_id]=${ownerId}&sort=-name&per_page=100`,
    );
    expect(status).toBe(200);
    expect(body.data.map((c) => c.name)).toEqual(names);

    const flat = await getPage(`/api/v1/companies?owner_id=${ownerId}&per_page=100`);
    expect(flat.body.meta.total).toBe(live.length);
  });

  it("filters contacts, and the CSV export, with filter[status]", async () => {
    const live = db.contacts.filter((c) => !c.deleted_at);
    const status = live[0]!.status;
    const matching = live.filter((c) => c.status === status);
    expect(matching.length).toBeLessThan(live.length);

    const { body } = await getPage<{ status: string }>(`/api/v1/contacts?filter[status]=${status}&per_page=100`);
    expect(body.meta.total).toBe(matching.length);
    expect(body.data.every((c) => c.status === status)).toBe(true);

    const csv = await (await call("GET", `/api/v1/contacts/export?filter[status]=${status}`)).text();
    expect(csv.trim().split("\r\n")).toHaveLength(matching.length + 1);
    const flat = await (await call("GET", `/api/v1/contacts/export?status=${status}`)).text();
    expect(flat.trim().split("\r\n")).toHaveLength(live.length + 1);
  });

  it("filters activities with filter[kind] and filter[completed]", async () => {
    const all = await getPage<unknown>("/api/v1/activities?per_page=100");
    const flat = await getPage<unknown>("/api/v1/activities?kind=task&completed=false&per_page=100");
    expect(flat.body.meta.total).toBe(all.body.meta.total);

    const open = await getPage<{ kind: string; completed_at: string | null }>(
      "/api/v1/activities?filter[kind]=task&filter[completed]=false&per_page=100",
    );
    expect(open.body.meta.total).toBeGreaterThan(0);
    expect(open.body.meta.total).toBeLessThan(all.body.meta.total);
    expect(open.body.data.every((a) => a.kind === "task" && a.completed_at === null)).toBe(true);
  });

  it("refuses a filter, a search or a sort the list does not have, and a page size over 100", async () => {
    const filter = await getPage("/api/v1/deals?filter[bogus]=1");
    expect(filter.status).toBe(400);
    expect(Object.keys(filter.body.errors ?? {})).toEqual(["filter[bogus]"]);

    const search = await getPage("/api/v1/activities?search=acme");
    expect(search.status).toBe(400);
    expect(Object.keys(search.body.errors ?? {})).toEqual(["search"]);

    const sort = await getPage("/api/v1/contacts?sort=last_name");
    expect(sort.status).toBe(400);
    expect(Object.keys(sort.body.errors ?? {})).toEqual(["sort"]);

    const tooBig = await getPage("/api/v1/companies?per_page=101");
    expect(tooBig.status).toBe(400);
    expect(Object.keys(tooBig.body.errors ?? {})).toEqual(["per_page"]);

    expect((await getPage("/api/v1/companies?per_page=100")).status).toBe(200);
  });
});
