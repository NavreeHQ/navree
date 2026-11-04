import { errorMessage, isNavreeError, NavreeError } from "@navree/client";
import { describe, expect, it, vi } from "vitest";
import { createApiClient, unwrap } from "@/lib/api/client";
import { applyFieldErrors } from "@/lib/api/form-errors";

const BASE = "http://crm.test";

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json", "X-Request-Id": "req-1" },
  });

/** Navree error body. */
const nest = (statusCode: number, message: string | string[], error: string, extra: object = {}) =>
  json({ statusCode, message, error, ...extra }, statusCode);

function recordingClient(respond: (request: Request) => Response | Promise<Response>) {
  const requests: Request[] = [];
  const client = createApiClient({
    baseUrl: BASE,
    fetch: async (request) => {
      requests.push(request.clone());
      return respond(request);
    },
  });
  return { client, requests };
}

describe("API client (browser → BFF)", () => {
  it("calls the BFF same-origin with Accept: application/json and no token or CSRF headers", async () => {
    const { client, requests } = recordingClient(() => json({ user: { id: "u1" } }));

    await unwrap(client.POST("/api/auth/login", { body: { email: "rep1@navree.test", password: "password123" } }));

    const [login] = requests;
    expect(login.url).toBe(`${BASE}/api/auth/login`);
    expect(login.credentials).toBe("same-origin");
    expect(login.headers.get("Accept")).toBe("application/json");
    expect(login.headers.get("Authorization")).toBeNull();
    expect(login.headers.get("X-XSRF-TOKEN")).toBeNull();
    expect(await login.json()).toEqual({ email: "rep1@navree.test", password: "password123" });
  });

  it("returns data for successful requests", async () => {
    const { client } = recordingClient(() => json({ count: 3 }));

    expect(await unwrap(client.GET("/api/notifications/unread-count"))).toEqual({ count: 3 });
  });

  it("sends a request id, so a failure can be found in the API's logs", async () => {
    const { client, requests } = recordingClient(() => json({ count: 0 }));

    await unwrap(client.GET("/api/notifications/unread-count"));

    expect(requests[0].headers.get("X-Request-Id")).toMatch(/^[0-9a-f-]{36}$/);
  });
});

describe("Navree error mapping", () => {
  it("turns a 400 validation error into a NavreeError with field errors", async () => {
    const { client } = recordingClient(() =>
      nest(400, ["name should not be empty", "website must be a URL address"], "Bad Request", {
        errors: { name: ["name should not be empty"], website: ["website must be a URL address"] },
      }),
    );

    const error = await unwrap(client.POST("/api/v1/companies", { body: { name: "" } })).catch((e) => e);

    expect(error).toBeInstanceOf(NavreeError);
    expect(error.status).toBe(400);
    expect(error.code).toBe("validation_failed");
    expect(error.isValidation).toBe(true);
    expect(error.requestId).toBe("req-1");
    expect(error.messages).toEqual(["name should not be empty", "website must be a URL address"]);
    expect(error.errors).toEqual({
      name: ["name should not be empty"],
      website: ["website must be a URL address"],
    });
  });

  it("maps 400 `errors` onto form fields, and unknown fields to the form root", () => {
    const error = new NavreeError({
      statusCode: 400,
      message: ["name should not be empty", "owner_id must be a UUID"],
      error: "Bad Request",
      errors: { name: ["name should not be empty"], owner_id: ["owner_id must be a UUID"] },
    });
    const setFieldError = vi.fn();

    // The form-level message comes back for the form's alert.
    expect(applyFieldErrors(error, setFieldError, ["name", "domain"])).toBe("owner_id must be a UUID");
    expect(setFieldError).toHaveBeenCalledWith("name", "name should not be empty");
    expect(setFieldError).toHaveBeenCalledTimes(1);
  });

  it("renames API fields to form fields (company_id → company)", () => {
    const error = new NavreeError({
      statusCode: 400,
      message: ["company_id must exist"],
      error: "Bad Request",
      errors: { company_id: ["company_id must exist"] },
    });
    const setFieldError = vi.fn();

    expect(applyFieldErrors(error, setFieldError, { company_id: "company" })).toBe("");
    expect(setFieldError).toHaveBeenCalledWith("company", "company_id must exist");
  });

  it("shows a 400 with only a message list at the form root", () => {
    const error = new NavreeError({ statusCode: 400, message: ["stage must be a valid stage"], error: "Bad Request" });
    const setFieldError = vi.fn();

    expect(applyFieldErrors(error, setFieldError, [])).toBe("stage must be a valid stage");
    expect(setFieldError).not.toHaveBeenCalled();
  });

  it("leaves non-validation errors alone", () => {
    const setFieldError = vi.fn();
    const forbidden = new NavreeError({ statusCode: 403, message: "Forbidden resource", error: "Forbidden" });
    const conflict = new NavreeError({ statusCode: 400, message: "Bad Request", error: "Bad Request" });

    expect(applyFieldErrors(forbidden, setFieldError, [])).toBeNull();
    expect(applyFieldErrors(conflict, setFieldError, [])).toBeNull();
    expect(applyFieldErrors(new Error("boom"), setFieldError, [])).toBeNull();
    expect(setFieldError).not.toHaveBeenCalled();
  });

  it("notifies on 401 (go to /login) and 403 (toast)", async () => {
    const onUnauthorized = vi.fn();
    const onForbidden = vi.fn();
    let status = 401;
    const client = createApiClient({
      baseUrl: BASE,
      fetch: async () =>
        status === 401
          ? nest(401, "Unauthorized", "Unauthorized")
          : nest(403, "Only the deal's owner can move it", "Forbidden"),
      onUnauthorized,
      onForbidden,
    });

    await client.GET("/api/auth/me");
    expect(onUnauthorized).toHaveBeenCalledTimes(1);

    status = 403;
    const error = await unwrap(
      client.POST("/api/v1/deals/{id}/move", { params: { path: { id: "d1" } }, body: { stage: "won" } }),
    ).catch((e) => e);
    expect(onForbidden).toHaveBeenCalledTimes(1);
    expect(onForbidden.mock.calls[0][0]).toBeInstanceOf(NavreeError);
    expect(onForbidden.mock.calls[0][0].messages).toEqual(["Only the deal's owner can move it"]);
    expect(error.status).toBe(403);
  });

  it("builds an error for non-JSON bodies", async () => {
    const client = createApiClient({
      baseUrl: BASE,
      fetch: async () => new Response("upstream timeout", { status: 504, statusText: "Gateway Timeout" }),
    });

    const error = await unwrap(client.GET("/api/v1/dashboard")).catch((e) => e);

    expect(error).toBeInstanceOf(NavreeError);
    expect(error.status).toBe(504);
    expect(error.messages).toEqual(["upstream timeout"]);
  });

  it("turns a request that got no answer into a network error", async () => {
    const client = createApiClient({
      baseUrl: BASE,
      fetch: async () => {
        throw new TypeError("Failed to fetch");
      },
    });

    const error = await unwrap(client.GET("/api/v1/dashboard")).catch((e) => e);

    expect(isNavreeError(error)).toBe(true);
    expect(error.status).toBe(0);
    expect(error.code).toBe("network_error");
    expect(errorMessage(error)).toBe("Can't reach the server. Check your connection.");
  });

  it("gives people readable messages instead of framework defaults", () => {
    const err = (statusCode: number, message: string | string[], error: string, errors?: Record<string, string[]>) =>
      new NavreeError({ statusCode, message, error, ...(errors ? { errors } : {}) });

    expect(errorMessage(err(403, "Forbidden resource", "Forbidden"))).toBe("You don't have permission to do that.");
    expect(errorMessage(err(403, "Won and lost deals are kept for reporting", "Forbidden"))).toBe(
      "Won and lost deals are kept for reporting",
    );
    expect(errorMessage(err(404, "Not Found", "Not Found"))).toBe("That record no longer exists.");
    expect(
      errorMessage(err(400, ["lost_reason should not be empty"], "Bad Request", { lost_reason: ["Say why"] })),
    ).toBe("Say why");
    expect(errorMessage(err(409, "That email is already taken", "Conflict"))).toBe("That email is already taken");
    expect(errorMessage(err(500, "Internal server error", "Internal Server Error"))).toBe(
      "The server hit an error. Try again in a moment.",
    );
  });
});
