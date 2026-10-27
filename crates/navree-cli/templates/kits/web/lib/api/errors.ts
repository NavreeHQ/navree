/**
 * The API's errors: every error response is `{statusCode, message, error, code}`, plus
 * `errors: {field: [messages]}` for validation errors (400, `code: "validation_failed"`).
 */

/** What to show for an error `code` (the place to plug in translations). */
const MESSAGES: Record<string, string> = {
  invalid_credentials: "Wrong email or password.",
  too_many_login_attempts: "Too many attempts. Wait a minute and try again.",
  invalid_two_factor_code: "That code is not right. Try the next one from your app.",
  two_factor_challenge_expired: "Your sign-in attempt expired. Sign in again.",
  not_a_member: "You are not a member of this workspace.",
  workspace_not_found: "This workspace does not exist.",
  invitation_expired: "This invitation has expired. Ask for a new one.",
  invitation_email_mismatch: "This invitation was sent to another email address.",
  already_a_member: "They are already a member of this workspace.",
  owner_required: "The owner can't be removed or demoted; transfer ownership first.",
  api_unreachable: "The server is unreachable. Try again in a moment.",
  invalid_origin: "This request came from another site and was refused.",
};

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
    readonly errors: Record<string, string[]> = {},
  ) {
    super(message);
    this.name = "ApiError";
  }

  /** The error of a failed response (any body). */
  static async from(response: Response): Promise<ApiError> {
    let body: { message?: unknown; code?: unknown; errors?: unknown } | null = null;
    try {
      body = (await response.json()) as typeof body;
    } catch {
      // not JSON
    }
    const message = Array.isArray(body?.message)
      ? body.message.join(", ")
      : typeof body?.message === "string"
        ? body.message
        : response.statusText || `Request failed (${response.status})`;
    const errors =
      body?.errors && typeof body.errors === "object" ? (body.errors as Record<string, string[]>) : {};
    return new ApiError(response.status, typeof body?.code === "string" ? body.code : "", message, errors);
  }

  /** A 400 whose `errors` name the fields. */
  get isValidation(): boolean {
    return this.status === 400 && Object.keys(this.errors).length > 0;
  }
}

export function isApiError(error: unknown): error is ApiError {
  return error instanceof ApiError;
}

/** A sentence for the person: the code's message when there is one, else the API's. */
export function errorMessage(error: unknown): string {
  if (isApiError(error)) return MESSAGES[error.code] ?? error.message;
  if (error instanceof Error) return error.message;
  return "Something went wrong.";
}

/**
 * Shows the API's field errors under the form's fields (react-hook-form's `setError`); `true`
 * when there were any.
 */
export function applyFieldErrors<T extends string>(
  error: unknown,
  setError: (name: T, error: { type: string; message: string }) => void,
): boolean {
  if (!isApiError(error) || !error.isValidation) return false;
  for (const [field, messages] of Object.entries(error.errors)) {
    setError(field as T, { type: "server", message: messages[0] ?? "Invalid value" });
  }
  return true;
}
