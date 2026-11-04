import { isNavreeError } from "@navree/client";

function isFieldList<F extends string>(fields: Readonly<Record<string, F>> | readonly F[]): fields is readonly F[] {
  return Array.isArray(fields);
}

/**
 * Maps a 400 validation error (`errors: { field: [message] }`) onto a form.
 *
 * `setFieldError(field, message)` is VeeValidate's `setFieldError`. `fields`
 * maps API field names to form field names (`company_id` → `company`); a field
 * the form doesn't know about, or a validation message without a field, is
 * returned as the form's root message so it still shows.
 *
 * Returns `null` when the error was not a validation error (the caller shows a
 * toast), `""` when every message landed on a field, or the root message.
 */
export function applyFieldErrors<const F extends string>(
  error: unknown,
  setFieldError: (field: NoInfer<F>, message: string) => void,
  fields: Readonly<Record<string, F>> | readonly F[],
): string | null {
  if (!isNavreeError(error) || !error.isValidation) return null;
  const map: Readonly<Record<string, F>> = isFieldList(fields)
    ? (Object.fromEntries(fields.map((f) => [f, f])) as Record<string, F>)
    : fields;
  const entries = Object.entries(error.errors);
  const unknown: string[] = [];
  for (const [field, messages] of entries) {
    const message = messages[0] ?? "Invalid value.";
    const target = map[field];
    if (target) setFieldError(target, message);
    else unknown.push(message);
  }
  if (unknown.length > 0 || entries.length === 0) {
    return unknown[0] ?? error.messages[0] ?? "Check the highlighted fields.";
  }
  return "";
}
