/** Validation errors from the API onto form fields (vee-validate's `setFieldError`, or anything with its signature). */
import { isNavreeError } from "@navree/client";

export interface ApplyFieldErrorsOptions {
  /** The form's fields; messages for other fields go to `onOther`. */
  fields?: readonly string[];
  /** Messages without a field of the form (show them above the form or in a toast). */
  onOther?: (message: string) => void;
}

/**
 * Puts a 400 validation error's messages (`errors: {field: [message]}`) on
 * the form's fields. Returns true when `error` was a validation error.
 *
 * ```ts
 * const { setFieldError } = useForm(...);
 * onError: (error) => applyFieldErrors(error, setFieldError, { onOther: toast.error }) || toast.error(errorMessage(error))
 * ```
 */
export function applyFieldErrors(
  error: unknown,
  setFieldError: (field: string, message: string) => void,
  options: ApplyFieldErrorsOptions = {},
): boolean {
  if (!isNavreeError(error) || !error.isValidation) return false;
  const entries = Object.entries(error.errors);
  for (const [field, messages] of entries) {
    const message = messages[0] ?? "Invalid value.";
    if (options.fields && !options.fields.includes(field)) options.onOther?.(message);
    else setFieldError(field, message);
  }
  if (entries.length === 0) options.onOther?.(error.messages[0] ?? "Check the highlighted fields.");
  return true;
}
