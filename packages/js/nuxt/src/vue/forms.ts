/** Validation errors from the API onto form fields (vee-validate's `setFieldError`, or anything with its signature). */
import { isNavreeError } from "@navree/client";

type FieldMap = readonly string[] | Readonly<Record<string, string>>;

function isFieldList(fields: FieldMap): fields is readonly string[] {
  return Array.isArray(fields);
}

export interface ApplyFieldErrorsOptions {
  /**
   * The form's fields. A list names the fields whose API name is the form's name; an object maps
   * the API's field names to the form's (`{ company_id: "company" }`). Messages for any other field
   * go to `onOther`. Without this option every field is set under the name the API used.
   */
  fields?: FieldMap;
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
 *
 * When the API names a field differently from the form, say how:
 *
 * ```ts
 * applyFieldErrors(error, setFieldError, { fields: { company_id: "company", owner_id: "owner" }, onOther: toast.error })
 * ```
 */
export function applyFieldErrors(
  error: unknown,
  setFieldError: (field: string, message: string) => void,
  options: ApplyFieldErrorsOptions = {},
): boolean {
  if (!isNavreeError(error) || !error.isValidation) return false;
  const { fields } = options;
  const target = (field: string): string | undefined => {
    if (!fields) return field;
    if (isFieldList(fields)) return fields.includes(field) ? field : undefined;
    return Object.hasOwn(fields, field) ? fields[field] : undefined;
  };
  const entries = Object.entries(error.errors);
  for (const [field, messages] of entries) {
    const message = messages[0] ?? "Invalid value.";
    const name = target(field);
    if (name === undefined) options.onOther?.(message);
    else setFieldError(name, message);
  }
  if (entries.length === 0) options.onOther?.(error.messages[0] ?? "Check the highlighted fields.");
  return true;
}
