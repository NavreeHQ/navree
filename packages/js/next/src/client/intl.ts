/**
 * Locales: `<html lang dir>` for right-to-left languages, and error
 * messages translated by error code (with next-intl's `t`, or any translate
 * function).
 */
import { direction } from "@navree/client";

export { direction, errorKey, isRtl, translateError, type Translate } from "@navree/client";

/** `<html {...localeAttributes(locale)}>`: `lang` and `dir`. */
export function localeAttributes(locale: string): { lang: string; dir: "rtl" | "ltr" } {
  return { lang: locale, dir: direction(locale) };
}
