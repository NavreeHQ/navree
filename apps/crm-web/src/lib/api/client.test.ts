import { describe, expect, it, vi } from "vitest";
import { createApiClient, unwrap } from "./client";
import { ApiError, errorMessage } from "./errors";
import { applyFieldErrors } from "./form-errors";

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
});

describe("Navree error mapping", () => {
  it("turns a 400 validation error into an ApiError with field errors", async () => {
    const { client } = recordingClient(() =>
      nest(400, ["name should not be empty", "website must be a URL address"], "Bad Request", {
        errors: { name: ["name should not be empty"], website: ["website must be a URL address"] },
      }),
    );

    const error = await unwrap(client.POST("/api/v1/companies", { body: { name: "" } })).catch((e) => e);

    expect(error).toBeInstanceOf(ApiError);
    expect(error.status).toBe(400);
    expect(error.isValidation).toBe(true);
    expect(error.requestId).toBe("req-1");
    expect(error.messages).toEqual(["name should not be empty", "website must be a URL address"]);
    expect(error.fieldErrors).toEqual({
      name: ["name should not be empty"],
      website: ["website must be a URL address"],
    });
  });

  it("maps 400 `errors` onto form fields, and unknown fields to the form root", () => {
    const error = new ApiError({
      statusCode: 400,
      message: ["name should not be empty", "owner_id must be a UUID"],
      error: "Bad Request",
      errors: { name: ["name should not be empty"], owner_id: ["owner_id must be a UUID"] },
    });
    const setError = vi.fn();

    expect(applyFieldErrors(error, setError, ["name", "domain"])).toBe(true);
    expect(setError).toHaveBeenCalledWith("name", { type: "server", message: "name should not be empty" });
    expect(setError).toHaveBeenCalledWith("root.server", { type: "server", message: "owner_id must be a UUID" });
  });

  it("shows a 400 with only a message list at the form root", () => {
    const error = new ApiError({ statusCode: 400, message: ["stage must be a valid stage"], error: "Bad Request" });
    const setError = vi.fn();

    expect(applyFieldErrors(error, setError)).toBe(true);
    expect(setError).toHaveBeenCalledWith("root.server", { type: "server", message: "stage must be a valid stage" });
  });

  it("leaves non-validation errors alone", () => {
    const setError = vi.fn();
    const forbidden = new ApiError({ statusCode: 403, message: "Forbidden resource", error: "Forbidden" });
    const conflict = new ApiError({ statusCode: 400, message: "Bad Request", error: "Bad Request" });

    expect(applyFieldErrors(forbidden, setError)).toBe(false);
    expect(applyFieldErrors(conflict, setError)).toBe(false);
    expect(applyFieldErrors(new Error("boom"), setError)).toBe(false);
    expect(setError).not.toHaveBeenCalled();
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
    expect(onForbidden.mock.calls[0][0]).toBeInstanceOf(ApiError);
    expect(onForbidden.mock.calls[0][0].messages).toEqual(["Only the deal's owner can move it"]);
    expect(error.status).toBe(403);
  });

  it("builds an error for non-JSON bodies", async () => {
    const client = createApiClient({
      baseUrl: BASE,
      fetch: async () => new Response("upstream timeout", { status: 504, statusText: "Gateway Timeout" }),
    });

    const error = await unwrap(client.GET("/api/v1/dashboard")).catch((e) => e);

    expect(error).toBeInstanceOf(ApiError);
    expect(error.status).toBe(504);
    expect(error.messages).toEqual(["upstream timeout"]);
  });

  it("gives people readable messages instead of framework defaults", () => {
    const err = (statusCode: number, message: string | string[], error: string, errors?: Record<string, string[]>) =>
      new ApiError({ statusCode, message, error, ...(errors ? { errors } : {}) });

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

  it("reads the machine-readable code and shows the message for known codes", () => {
    const wrong = new ApiError({
      statusCode: 401,
      message: "Invalid credentials",
      error: "Unauthorized",
      code: "invalid_credentials",
    });
    expect(wrong.code).toBe("invalid_credentials");
    expect(errorMessage(wrong)).toBe("That email and password don't match an account. Check them and try again.");

    const demo = new ApiError({
      statusCode: 403,
      message: "Demo workspaces are read-only.",
      error: "Forbidden",
      code: "demo_read_only",
    });
    expect(errorMessage(demo)).toBe("This is a demo workspace, so changes aren't saved.");

    // A code without its own message keeps the status rules and the API's detail.
    const taken = new ApiError({
      statusCode: 409,
      message: "That email is already taken",
      error: "Conflict",
      code: "conflict",
    });
    expect(errorMessage(taken)).toBe("That email is already taken");

    // With `http.errors.code` off, there is no code.
    expect(new ApiError({ statusCode: 404, message: "Not Found", error: "Not Found" }).code).toBeUndefined();
  });

  it("keeps the code of an error body and has none for a non-JSON error", async () => {
    const body = {
      statusCode: 409,
      message: "The campaign is already running.",
      error: "Conflict",
      code: "campaign_already_running",
    };
    const response = new Response(JSON.stringify(body), { status: 409, headers: { "x-request-id": "abc" } });
    const error = ApiError.fromResponse(response, body);
    expect(error.code).toBe("campaign_already_running");
    expect(error.requestId).toBe("abc");

    const plain = ApiError.fromResponse(new Response("upstream timeout", { status: 504 }), "upstream timeout");
    expect(plain.code).toBeUndefined();
  });
});
