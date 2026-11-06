import { isNavreeError } from "@navree/client";
import { applyFieldErrors as applyToForm } from "@navree/nuxt/vue";

/**
 * Maps a 400 validation error (`errors: { field: [message] }`) onto a form.
 *
 * `setFieldError(field, message)` is VeeValidate's `setFieldError`. `fields`
 * maps API field names to form field names (`company_id` → `company`); a field
 * the form doesn't know about, or a validation message without a field, is
 * returned as the form's root message so it still shows. The mapping is the
 * module's `applyFieldErrors`; this wrapper gives the forms one message back.
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
  const others: string[] = [];
  applyToForm(error, (field, message) => setFieldError(field as NoInfer<F>, message), {
    fields,
    onOther: (message) => others.push(message),
  });
  return others[0] ?? "";
}
