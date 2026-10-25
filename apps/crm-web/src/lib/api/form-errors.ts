import type { FieldPath, FieldValues, UseFormSetError } from "react-hook-form";
import { isApiError } from "./errors";

/**
 * Maps a 400 validation error (`errors: { field: [message] }`) onto form fields
 * (react-hook-form's `setError`). Fields the form doesn't know about, and
 * validation messages without a field, go to `root.server` so they still show.
 *
 * Returns true when the error was a validation error and has been applied.
 */
export function applyFieldErrors<T extends FieldValues>(
  error: unknown,
  setError: UseFormSetError<T>,
  knownFields?: readonly string[],
): boolean {
  if (!isApiError(error) || !error.isValidation) return false;
  const entries = Object.entries(error.fieldErrors);
  const unknown: string[] = [];
  for (const [field, messages] of entries) {
    const message = messages[0] ?? "Invalid value.";
    if (knownFields && !knownFields.includes(field)) {
      unknown.push(message);
      continue;
    }
    setError(field as FieldPath<T>, { type: "server", message });
  }
  if (unknown.length > 0 || entries.length === 0) {
    setError("root.server", {
      type: "server",
      message: unknown[0] ?? error.messages[0] ?? "Check the highlighted fields.",
    });
  }
  return true;
}
