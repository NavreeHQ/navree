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
      email: "riley.novak@navis.test",
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
