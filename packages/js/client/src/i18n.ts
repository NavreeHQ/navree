/**
 * Locale helpers shared by the Next.js and Nuxt packages: text direction
 * for `<html dir>`, and error messages translated by error code.
 */
import { errorMessage, isNavreeError } from "./errors";

/** Languages written right to left (ISO 639 codes). */
export const RTL_LANGUAGES: ReadonlySet<string> = new Set([
  "ar",
  "arc",
  "ckb",
  "dv",
  "fa",
  "he",
  "iw",
  "ks",
  "ps",
  "sd",
  "syr",
  "ug",
  "ur",
  "yi",
]);

/** Scripts written right to left (ISO 15924 codes, as locale subtags). */
const RTL_SCRIPTS = new Set(["arab", "hebr", "thaa", "syrc", "nkoo", "adlm", "rohg", "mand", "samr"]);

/** True when `locale` (`ar`, `ar-AE`, `pa-Arab`) is written right to left. */
export function isRtl(locale: string | undefined | null): boolean {
  if (!locale) return false;
  const [language = "", ...subtags] = locale.toLowerCase().split(/[-_]/);
  const script = subtags.find((tag) => tag.length === 4 && /^[a-z]{4}$/.test(tag));
  if (script) return RTL_SCRIPTS.has(script);
  return RTL_LANGUAGES.has(language);
}

/** The `dir` attribute for `locale`. */
export function direction(locale: string | undefined | null): "rtl" | "ltr" {
  return isRtl(locale) ? "rtl" : "ltr";
}

/** The message key of an error: `errors.<code>` (`errors.network_error` offline, `errors.unknown` otherwise). */
export function errorKey(error: unknown, prefix = "errors"): string {
  if (isNavreeError(error)) return `${prefix}.${error.code}`;
  if (error instanceof TypeError) return `${prefix}.network_error`;
  return `${prefix}.unknown`;
}

/** A translate function: next-intl's `t` (with `t.has`) or vue-i18n's `t`. */
export type Translate = ((key: string, values?: Record<string, string | number>) => string) & {
  has?: (key: string) => boolean;
};

export interface TranslateErrorOptions {
  /** Whether a key exists (vue-i18n's `te`); defaults to `t.has` when the translator has it. */
  has?: (key: string) => boolean;
  /** The key prefix (default `errors`). */
  prefix?: string;
  /** The text when nothing better is known. */
  fallback?: string;
}

/**
 * The error's message in the person's language: the catalogue's message for
 * its code (`errors.limit_reached`), else the catalogue's `errors.unknown`,
 * else the English sentence {@link errorMessage} picks. The message gets
 * `{status}` and the API's `{message}` as values.
 */
export function translateError(error: unknown, t: Translate, options: TranslateErrorOptions = {}): string {
  const prefix = options.prefix ?? "errors";
  const has = options.has ?? (t.has ? (key: string) => t.has!(key) : undefined);
  const values = isNavreeError(error) ? { status: error.status, message: error.message } : undefined;
  const key = errorKey(error, prefix);
  if (!has) return t(key, values);
  if (has(key)) return t(key, values);
  const unknown = `${prefix}.unknown`;
  if (has(unknown)) return t(unknown, values);
  return errorMessage(error, options.fallback);
}
