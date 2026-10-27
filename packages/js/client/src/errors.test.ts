import { describe, expect, it } from "vitest";
import { errorMessage, isErrorBody, isNavreeError, NavreeError } from "./errors";

const response = (status: number, headers: Record<string, string> = {}, statusText = "") =>
  new Response(null, { status, statusText, headers });

describe("NavreeError.fromResponse", () => {
  it("keeps every member of Navree's error body", () => {
    const error = NavreeError.fromResponse(response(409, { "x-request-id": "req-9" }), {
      statusCode: 409,
      message: "The invoice has already been sent.",
      error: "Conflict",
      code: "invoice_already_sent",
    });

    expect(error).toBeInstanceOf(NavreeError);
    expect(error).toBeInstanceOf(Error);
    expect(error.name).toBe("NavreeError");
    expect(error.status).toBe(409);
    expect(error.code).toBe("invoice_already_sent");
    expect(error.message).toBe("The invoice has already been sent.");
    expect(error.messages).toEqual(["The invoice has already been sent."]);
    expect(error.errors).toEqual({});
    expect(error.requestId).toBe("req-9");
    expect(error.body.error).toBe("Conflict");
    expect(error.is("invoice_already_sent")).toBe(true);
    expect(error.is("conflict", "invoice_already_sent")).toBe(true);
    expect(error.is("not_found")).toBe(false);
  });

  it("reads a validation error: every message, the field errors, validation_failed", () => {
    const error = NavreeError.fromResponse(response(400), {
      statusCode: 400,
      message: ["email must be a valid email address", "name should not be empty"],
      error: "Bad Request",
      code: "validation_failed",
      errors: { email: ["email must be a valid email address"], name: ["name should not be empty"] },
    });

    expect(error.isValidation).toBe(true);
    expect(error.message).toBe("email must be a valid email address; name should not be empty");
    expect(error.messages).toHaveLength(2);
    expect(error.errors.email).toEqual(["email must be a valid email address"]);
    expect(error.fieldError("name")).toBe("name should not be empty");
    expect(error.fieldError("phone")).toBeUndefined();
  });

  it("derives the code from the status when the API sends none (older APIs, or http.errors.code off)", () => {
    const body = (statusCode: number, error: string, extra: object = {}) => ({
      statusCode,
      message: error,
      error,
      ...extra,
    });
    expect(NavreeError.fromResponse(response(404), body(404, "Not Found")).code).toBe("not_found");
    expect(NavreeError.fromResponse(response(401), body(401, "Unauthorized")).code).toBe("unauthenticated");
    expect(NavreeError.fromResponse(response(500), body(500, "Internal Server Error")).code).toBe("internal_error");
    expect(NavreeError.fromResponse(response(429), body(429, "Too Many Requests")).code).toBe("too_many_requests");
    expect(
      NavreeError.fromResponse(response(400), body(400, "Bad Request", { errors: { a: ["x"] } })).code,
    ).toBe("validation_failed");
    expect(NavreeError.fromResponse(response(400), body(400, "Bad Request")).code).toBe("bad_request");
  });

  it("builds an error from a response whose body is not Navree's shape", () => {
    const html = NavreeError.fromResponse(response(502, {}, "Bad Gateway"), "<html>proxy error</html>");
    expect(html.status).toBe(502);
    expect(html.code).toBe("bad_gateway");
    expect(html.message).toBe("Bad Gateway");

    const text = NavreeError.fromResponse(response(503), "The database is restarting");
    expect(text.message).toBe("The database is restarting");
    expect(text.code).toBe("service_unavailable");

    const empty = NavreeError.fromResponse(response(418), undefined);
    expect(empty.message).toBe("Request failed");
    expect(empty.code).toBe("http_418");
  });

  it("fills a missing statusCode from the response", () => {
    const error = NavreeError.fromResponse(response(403), {
      statusCode: 0,
      message: "Demo workspaces are read-only.",
      error: "Forbidden",
      code: "demo_read_only",
    });
    expect(error.status).toBe(403);
  });

  it("wraps a network failure as status 0 with network_error", () => {
    const cause = new TypeError("fetch failed");
    const error = NavreeError.network(cause);
    expect(error.status).toBe(0);
    expect(error.code).toBe("network_error");
    expect(error.cause).toBe(cause);
    expect(isNavreeError(error)).toBe(true);
  });

  it("serializes to Navree's body shape", () => {
    const error = NavreeError.fromResponse(response(404), {
      statusCode: 404,
      message: "Deal not found",
      error: "Not Found",
      code: "not_found",
    });
    expect(JSON.parse(JSON.stringify(error))).toEqual({
      statusCode: 404,
      message: "Deal not found",
      error: "Not Found",
      code: "not_found",
    });
  });
});

describe("isErrorBody", () => {
  it("accepts Navree bodies and rejects anything else", () => {
    expect(isErrorBody({ statusCode: 404, message: "x", error: "Not Found" })).toBe(true);
    expect(isErrorBody({ statusCode: 400, message: ["a", "b"], error: "Bad Request" })).toBe(true);
    expect(isErrorBody({ message: "x" })).toBe(false);
    expect(isErrorBody("x")).toBe(false);
    expect(isErrorBody(null)).toBe(false);
  });
});

describe("errorMessage", () => {
  const error = (statusCode: number, message: string | string[], extra: object = {}) =>
    NavreeError.fromResponse(response(statusCode), {
      statusCode,
      message,
      error: reason(statusCode),
      ...extra,
    });
  const reason = (status: number) =>
    ({ 400: "Bad Request", 401: "Unauthorized", 403: "Forbidden", 404: "Not Found", 409: "Conflict", 429: "Too Many Requests", 500: "Internal Server Error", 502: "Bad Gateway" })[status] ?? "Error";

  it("prefers the API's own detail", () => {
    expect(errorMessage(error(403, "Only the owner can archive a project."))).toBe(
      "Only the owner can archive a project.",
    );
    expect(errorMessage(error(409, "That email is taken."))).toBe("That email is taken.");
  });

  it("shows the first field message of a validation error", () => {
    expect(errorMessage(error(400, ["name should not be empty"], { errors: { name: ["name should not be empty"] } }))).toBe(
      "name should not be empty",
    );
  });

  it("falls back to a sentence a person can act on when the message is only the reason phrase", () => {
    expect(errorMessage(error(401, "Unauthorized"))).toBe("Your session has ended. Sign in again.");
    expect(errorMessage(error(403, "Forbidden"))).toBe("You don't have permission to do that.");
    expect(errorMessage(error(404, "Not Found"))).toBe("That record no longer exists.");
    expect(errorMessage(error(429, "Too Many Requests"))).toBe("Too many attempts. Wait a minute and try again.");
    expect(errorMessage(error(500, "Internal server error"))).toBe("The server hit an error. Try again in a moment.");
    expect(errorMessage(error(502, "Bad Gateway"))).toBe("Can't reach the API. Try again in a moment.");
  });

  it("handles network failures and unknown values", () => {
    expect(errorMessage(NavreeError.network(new TypeError("x")))).toBe("Can't reach the server. Check your connection.");
    expect(errorMessage(new TypeError("Failed to fetch"))).toBe("Can't reach the server. Check your connection.");
    expect(errorMessage("boom")).toBe("Something went wrong. Try again.");
    expect(errorMessage(undefined, "Could not save.")).toBe("Could not save.");
  });
});
