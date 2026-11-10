/**
 * Navree's JSON error shape, as every Navree API sends it:
 *
 * ```json
 * {"statusCode": 400, "message": ["email must be a valid email address"],
 *  "error": "Bad Request", "code": "validation_failed",
 *  "errors": {"email": ["email must be a valid email address"]}}
 * ```
 *
 * - `statusCode` is the HTTP status;
 * - `message` is a sentence for people, or every field message for a validation error;
 * - `error` is the status's reason phrase;
 * - `code` is stable, lower snake case, and what a front end translates;
 * - `errors` (validation errors only) maps field paths to their messages.
 */
export interface NavreeErrorBody {
  statusCode: number;
  message: string | string[];
  error: string;
  code?: string;
  errors?: Record<string, string[]>;
}

const REASONS: Record<number, string> = {
  400: "Bad Request",
  401: "Unauthorized",
  402: "Payment Required",
  403: "Forbidden",
  404: "Not Found",
  405: "Method Not Allowed",
  409: "Conflict",
  410: "Gone",
  413: "Payload Too Large",
  415: "Unsupported Media Type",
  422: "Unprocessable Entity",
  429: "Too Many Requests",
  500: "Internal Server Error",
  502: "Bad Gateway",
  503: "Service Unavailable",
  504: "Gateway Timeout",
};

/** The reason phrase of an HTTP status (`404` → `"Not Found"`). */
export function reasonPhrase(status: number): string | undefined {
  return REASONS[status];
}

/**
 * The code Navree gives an error of `status` when nothing more specific
 * applies: the reason phrase in snake case, `unauthenticated` for 401,
 * `internal_error` for 500 and `validation_failed` for a 400 with field
 * errors. Used when a body carries no `code`.
 */
export function defaultCode(status: number, hasFieldErrors = false): string {
  if (status === 0) return "network_error";
  if (status === 400 && hasFieldErrors) return "validation_failed";
  if (status === 401) return "unauthenticated";
  if (status === 500) return "internal_error";
  const reason = REASONS[status];
  return reason ? reason.toLowerCase().replace(/[^a-z0-9]+/g, "_") : `http_${status}`;
}

/** True when `body` has Navree's error shape. */
export function isErrorBody(body: unknown): body is NavreeErrorBody {
  if (typeof body !== "object" || body === null) return false;
  const b = body as Record<string, unknown>;
  return typeof b.statusCode === "number" && (typeof b.message === "string" || Array.isArray(b.message));
}

/**
 * An error answer from a Navree API (or the backend-for-frontend in front of
 * it). `status`, `code`, `message`, `errors` and the `X-Request-Id` come
 * straight from the response; `code` falls back to the status's default
 * when the body has none.
 */
export class NavreeError extends Error {
  override readonly name = "NavreeError";
  /** The HTTP status; `0` when the request never got an answer. */
  readonly status: number;
  /** The machine-readable code (`not_found`, `validation_failed`, `invoice_already_sent`). */
  readonly code: string;
  /** Field errors of a validation failure (`{field: [message]}`); empty otherwise. */
  readonly errors: Record<string, string[]>;
  /** The response's `X-Request-Id`, for support and the logs. */
  readonly requestId: string | null;
  /** The error body as the API sent it (with `code` filled in). */
  readonly body: NavreeErrorBody;

  constructor(body: NavreeErrorBody, requestId: string | null = null, options?: { cause?: unknown }) {
    super(Array.isArray(body.message) ? body.message.join("; ") : body.message, options);
    this.status = body.statusCode;
    this.errors = body.errors ?? {};
    this.code = body.code ?? defaultCode(body.statusCode, body.errors !== undefined);
    this.body = { ...body, code: this.code };
    this.requestId = requestId;
  }

  /** `message` as a list (one entry, or every field message of a validation error). */
  get messages(): string[] {
    return Array.isArray(this.body.message) ? this.body.message : [this.body.message];
  }

  /** A 400 that carries field errors or a list of messages. */
  get isValidation(): boolean {
    return this.status === 400 && (this.body.errors !== undefined || Array.isArray(this.body.message));
  }

  /** The first message for `field`, if the API rejected it. */
  fieldError(field: string): string | undefined {
    return this.errors[field]?.[0];
  }

  /** True when the error's code is one of `codes`. */
  is(...codes: string[]): boolean {
    return codes.includes(this.code);
  }

  /** Navree's body shape, so an error can be logged or sent on as JSON. */
  toJSON(): NavreeErrorBody {
    return this.body;
  }

  /** The error for an answered request: Navree's body when there is one, else one built from the status. */
  static fromResponse(response: Response, body: unknown): NavreeError {
    const requestId = response.headers.get("x-request-id");
    if (isErrorBody(body)) {
      return new NavreeError({ ...body, statusCode: body.statusCode || response.status }, requestId);
    }
    const reason = response.statusText || reasonPhrase(response.status) || "Request failed";
    const message = typeof body === "string" && body.length > 0 && body.length < 300 && !body.startsWith("<") ? body : reason;
    return new NavreeError({ statusCode: response.status, message, error: reason }, requestId);
  }

  /** The error for a request that got no answer (offline, DNS, refused connection). */
  static network(cause: unknown): NavreeError {
    return new NavreeError(
      { statusCode: 0, message: "Network error", error: "Network Error", code: "network_error" },
      null,
      { cause },
    );
  }
}

/** True when `error` is a {@link NavreeError}. */
export function isNavreeError(error: unknown): error is NavreeError {
  return error instanceof NavreeError;
}

/** Built-in messages that only repeat the status and say nothing a person can act on. */
const GENERIC_MESSAGES = new Set([
  "Forbidden resource",
  "Internal server error",
  "Internal Server Error",
  "Unauthorized",
  "Forbidden",
  "Not Found",
  "Network error",
]);

function detail(error: NavreeError): string | undefined {
  const [first] = error.messages;
  if (error.messages.length !== 1 || first === undefined) return first;
  return first === error.body.error || GENERIC_MESSAGES.has(first) ? undefined : first;
}

/**
 * A sentence for a toast, in English: the API's own message when it says
 * something specific, else a sentence for the status. For other languages,
 * translate the error's `code` instead (see `translateError`).
 */
export function errorMessage(error: unknown, fallback = "Something went wrong. Try again."): string {
  if (isNavreeError(error)) {
    if (error.status === 0) return "Can't reach the server. Check your connection.";
    const specific = detail(error);
    if (error.status === 400) {
      const first = Object.values(error.errors)[0]?.[0];
      return first ?? specific ?? "Check the highlighted fields.";
    }
    if (error.status === 401) return "Your session has ended. Sign in again.";
    if (error.status === 403) return specific ?? "You don't have permission to do that.";
    if (error.status === 404) return specific ?? "That record no longer exists.";
    if (error.status === 409) return specific ?? "That conflicts with an existing record.";
    if (error.status === 429) return "Too many attempts. Wait a minute and try again.";
    if (error.status === 502 || error.status === 503 || error.status === 504) {
      return "Can't reach the API. Try again in a moment.";
    }
    if (error.status >= 500) return "The server hit an error. Try again in a moment.";
    return specific ?? fallback;
  }
  if (error instanceof TypeError) return "Can't reach the server. Check your connection.";
  return fallback;
}
