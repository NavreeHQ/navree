/**
 * Validation errors from the API onto form fields. Works with
 * react-hook-form's `setError` (or anything with its signature).
 */
import { isNavreeError } from "@navree/client";

type SetError = (name: string, error: { type: string; message: string }) => void;

/**
 * The form's fields: a list names the fields whose API name is the form's name; an object maps the
 * API's field names to the form's (`{ company_id: "company" }`).
 */
export type FieldMap = readonly string[] | Readonly<Record<string, string>>;

function isFieldList(fields: FieldMap): fields is readonly string[] {
  return Array.isArray(fields);
}

/**
 * Puts a 400 validation error's messages (`errors: {field: [message]}`) on
 * the form's fields. Fields the form doesn't know, and messages without a
 * field, go to `root.server` so they still show. Returns true when `error`
 * was a validation error.
 *
 * ```ts
 * onError: (error) => applyFieldErrors(error, form.setError, ["email", "name"]) || toast.error(errorMessage(error))
 * ```
 *
 * When the API names a field differently from the form, say how:
 *
 * ```ts
 * applyFieldErrors(error, form.setError, { company_id: "company", owner_id: "owner" })
 * ```
 */
export function applyFieldErrors(error: unknown, setError: SetError, knownFields?: FieldMap): boolean {
  if (!isNavreeError(error) || !error.isValidation) return false;
  const target = (field: string): string | undefined => {
    if (!knownFields) return field;
    if (isFieldList(knownFields)) return knownFields.includes(field) ? field : undefined;
    return Object.hasOwn(knownFields, field) ? knownFields[field] : undefined;
  };
  const entries = Object.entries(error.errors);
  const unknown: string[] = [];
  for (const [field, messages] of entries) {
    const message = messages[0] ?? "Invalid value.";
    const name = target(field);
    if (name === undefined) {
      unknown.push(message);
      continue;
    }
    setError(name, { type: "server", message });
  }
  if (unknown.length > 0 || entries.length === 0) {
    setError("root.server", { type: "server", message: unknown[0] ?? error.messages[0] ?? "Check the highlighted fields." });
  }
  return true;
}
