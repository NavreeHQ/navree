import type { ApiErrorBody } from "./types";

/**
 * An error response from the API, in the NestJS format:
 * `{statusCode, message: string | string[], error, errors?: {field: [messages]}}`.
 * Validation failures are 400 with `message: string[]` and `errors`.
 */
export class ApiError extends Error {
  readonly status: number;
  readonly body: ApiErrorBody;
  readonly requestId: string | null;

  constructor(body: ApiErrorBody, requestId: string | null = null) {
    super(Array.isArray(body.message) ? body.message.join("; ") : body.message);
    this.name = "ApiError";
    this.status = body.statusCode;
    this.body = body;
    this.requestId = requestId;
  }

  /** `message` as a list (NestJS sends a string, or a list for validation errors). */
  get messages(): string[] {
    return Array.isArray(this.body.message) ? this.body.message : [this.body.message];
  }

  /** Field errors from a validation failure (`errors: { field: [message] }`). */
  get fieldErrors(): Record<string, string[]> {
    return this.body.errors ?? {};
  }

  /** A 400 that carries per-field or list messages. */
  get isValidation(): boolean {
    return this.status === 400 && (this.body.errors !== undefined || Array.isArray(this.body.message));
  }

  static fromResponse(response: Response, body: unknown): ApiError {
    const requestId = response.headers.get("x-request-id");
    if (isErrorBody(body)) {
      return new ApiError({ ...body, statusCode: body.statusCode || response.status }, requestId);
    }
    const reason = response.statusText || defaultReason(response.status);
    return new ApiError(
      {
        statusCode: response.status,
        message: typeof body === "string" && body.length > 0 && body.length < 300 ? body : reason,
        error: reason,
      },
      requestId,
    );
  }
}

export function isApiError(error: unknown): error is ApiError {
  return error instanceof ApiError;
}

export function isErrorBody(body: unknown): body is ApiErrorBody {
  if (typeof body !== "object" || body === null) return false;
  const b = body as Record<string, unknown>;
  return typeof b.statusCode === "number" && (typeof b.message === "string" || Array.isArray(b.message));
}

function defaultReason(status: number): string {
  switch (status) {
    case 400:
      return "Bad Request";
    case 401:
      return "Unauthorized";
    case 403:
      return "Forbidden";
    case 404:
      return "Not Found";
    case 409:
      return "Conflict";
    case 429:
      return "Too Many Requests";
    default:
      return status >= 500 ? "Internal Server Error" : "Request failed";
  }
}

/** NestJS's built-in messages, which say nothing a person can act on. */
const GENERIC_MESSAGES = new Set(["Forbidden resource", "Internal server error", "Unauthorized", "Forbidden"]);

/** True when `message` is just the reason phrase or a framework default, i.e. carries no detail. */
function isGeneric(error: ApiError): boolean {
  const [first] = error.messages;
  return error.messages.length === 1 && (first === error.body.error || GENERIC_MESSAGES.has(first));
}

/** Human-readable message for toasts. */
export function errorMessage(error: unknown, fallback = "Something went wrong. Try again."): string {
  if (isApiError(error)) {
    const detail = isGeneric(error) ? undefined : error.messages[0];
    if (error.status === 400) {
      const first = Object.values(error.fieldErrors)[0]?.[0];
      return first ?? detail ?? "Check the highlighted fields.";
    }
    if (error.status === 401) return "Your session has ended. Sign in again.";
    if (error.status === 403) return detail ?? "You don't have permission to do that.";
    if (error.status === 404) return detail ?? "That record no longer exists.";
    if (error.status === 409) return detail ?? "That conflicts with an existing record.";
    if (error.status === 429) return "Too many attempts. Wait a minute and try again.";
    if (error.status === 502 || error.status === 503) return "Can't reach the Navis API. Try again in a moment.";
    if (error.status >= 500) return "The server hit an error. Try again in a moment.";
    return detail ?? fallback;
  }
  if (error instanceof TypeError) return "Can't reach the server. Check your connection.";
  return fallback;
}
