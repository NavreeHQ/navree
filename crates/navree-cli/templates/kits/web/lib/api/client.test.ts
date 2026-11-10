// @vitest-environment node
import { afterEach, describe, expect, it, vi } from "vitest";
import { api, apiEvents, listQuery, request, setWorkspace } from "./client";
import { ApiError, applyFieldErrors, errorMessage } from "./errors";

function recording(response: Response) {
  const calls: { url: string; init: RequestInit }[] = [];
  const fetch = vi.fn(async (url: RequestInfo | URL, init?: RequestInit) => {
    calls.push({ url: String(url), init: init ?? {} });
    return response.clone();
  }) as unknown as typeof globalThis.fetch;
  return { fetch, calls };
}

afterEach(() => {
  setWorkspace(null);
  apiEvents.onUnauthorized = undefined;
});

describe("the API client", () => {
  it("sends the current workspace as X-Workspace, and JSON bodies", async () => {
    const { fetch, calls } = recording(Response.json({ id: "p1" }, { status: 201 }));
    setWorkspace("agra");

    const created = await api.post<{ id: string }>("/api/v1/projects", { name: "Website" }, { fetch });

    expect(created).toEqual({ id: "p1" });
    const headers = calls[0].init.headers as Record<string, string>;
    expect(headers["x-workspace"]).toBe("agra");
    expect(headers["content-type"]).toBe("application/json");
    expect(calls[0].init.body).toBe('{"name":"Website"}');
  });

  it("can call outside the current workspace", async () => {
    const { fetch, calls } = recording(Response.json({ data: [] }));
    setWorkspace("agra");

    await api.get("/api/workspaces", { workspace: null, fetch });

    expect((calls[0].init.headers as Record<string, string>)["x-workspace"]).toBeUndefined();
  });

  it("returns nothing for a 204", async () => {
    const { fetch } = recording(new Response(null, { status: 204 }));
    await expect(api.delete("/api/v1/projects/p1", { fetch })).resolves.toBeUndefined();
  });

  it("throws the API's error with its code and field errors", async () => {
    const { fetch } = recording(
      Response.json(
        {
          statusCode: 400,
          message: ["name should not be empty"],
          error: "Bad Request",
          code: "validation_failed",
          errors: { name: ["name should not be empty"] },
        },
        { status: 400 },
      ),
    );

    const error = await request("POST", "/api/v1/projects", { name: "" }, { fetch }).catch((e: unknown) => e);

    expect(error).toBeInstanceOf(ApiError);
    const apiError = error as ApiError;
    expect(apiError.isValidation).toBe(true);
    expect(apiError.message).toBe("name should not be empty");
    const setError = vi.fn();
    expect(applyFieldErrors(apiError, setError)).toBe(true);
    expect(setError).toHaveBeenCalledWith("name", { type: "server", message: "name should not be empty" });
  });

  it("ends the session on a 401, except for the sign-in calls' own answers", async () => {
    const unauthorized = () =>
      recording(
        Response.json(
          { statusCode: 401, message: "Invalid credentials", error: "Unauthorized", code: "invalid_credentials" },
          { status: 401 },
        ),
      );
    const onUnauthorized = vi.fn();
    apiEvents.onUnauthorized = onUnauthorized;

    const login = await api.post("/api/auth/login", {}, { fetch: unauthorized().fetch }).catch((e: unknown) => e);
    expect(errorMessage(login)).toBe("Wrong email or password.");
    expect(onUnauthorized).not.toHaveBeenCalled();

    await api.get("/api/v1/projects", { fetch: unauthorized().fetch }).catch(() => undefined);
    expect(onUnauthorized).toHaveBeenCalledTimes(1);
  });

  it("sends a list request with filter[name]=value and no flat filter", async () => {
    const { fetch, calls } = recording(Response.json({ data: [], meta: { total: 0 } }));
    setWorkspace("agra");

    const list = listQuery({ filter: { status: "active", owner: undefined }, search: "web", page: 2, per_page: 20 });
    await api.get(`/api/v1/projects${list}`, { fetch });

    const sent = new URL(calls[0].url, "http://app.test");
    expect(sent.pathname).toBe("/api/v1/projects");
    expect(Object.fromEntries(sent.searchParams)).toEqual({
      "filter[status]": "active",
      search: "web",
      page: "2",
      per_page: "20",
    });
    expect(sent.searchParams.has("status")).toBe(false);
  });

  it("writes a list query in the canonical order and leaves out blank parts", () => {
    const text = listQuery({ filter: { status: "archived", kind: "" }, sort: "-created_at,name", search: " ", page: 1 });
    expect(text).toBe("?filter%5Bstatus%5D=archived&sort=-created_at%2Cname&page=1");
    expect(listQuery({})).toBe("");
  });
});
