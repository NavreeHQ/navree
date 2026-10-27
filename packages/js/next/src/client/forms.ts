/**
 * Validation errors from the API onto form fields. Works with
 * react-hook-form's `setError` (or anything with its signature).
 */
import { isNavreeError } from "@navree/client";

type SetError = (name: string, error: { type: string; message: string }) => void;

/**
 * Puts a 400 validation error's messages (`errors: {field: [message]}`) on
 * the form's fields. Fields the form doesn't know, and messages without a
 * field, go to `root.server` so they still show. Returns true when `error`
 * was a validation error.
 *
 * ```ts
 * onError: (error) => applyFieldErrors(error, form.setError, ["email", "name"]) || toast.error(errorMessage(error))
 * ```
 */
export function applyFieldErrors(error: unknown, setError: SetError, knownFields?: readonly string[]): boolean {
  if (!isNavreeError(error) || !error.isValidation) return false;
  const entries = Object.entries(error.errors);
  const unknown: string[] = [];
  for (const [field, messages] of entries) {
    const message = messages[0] ?? "Invalid value.";
    if (knownFields && !knownFields.includes(field)) {
      unknown.push(message);
      continue;
    }
    setError(field, { type: "server", message });
  }
  if (unknown.length > 0 || entries.length === 0) {
    setError("root.server", { type: "server", message: unknown[0] ?? error.messages[0] ?? "Check the highlighted fields." });
  }
  return true;
}
