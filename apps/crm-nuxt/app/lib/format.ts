/**
 * Formatting helpers. Money travels as integer minor units (`amount_cents`)
 * plus an ISO 4217 `currency`; the number of minor units comes from Intl, so
 * zero-decimal currencies (JPY, KRW) are handled correctly.
 */

const DEFAULT_LOCALE = "en-IN";

const formatters = new Map<string, Intl.NumberFormat>();

function numberFormat(locale: string, options: Intl.NumberFormatOptions): Intl.NumberFormat {
  const key = `${locale}|${JSON.stringify(options)}`;
  let formatter = formatters.get(key);
  if (!formatter) {
    formatter = new Intl.NumberFormat(locale, options);
    formatters.set(key, formatter);
  }
  return formatter;
}

/** Number of minor-unit digits for a currency (INR 2, JPY 0, BHD 3). */
export function currencyDigits(currency: string): number {
  try {
    return numberFormat(DEFAULT_LOCALE, { style: "currency", currency }).resolvedOptions().maximumFractionDigits ?? 2;
  } catch {
    return 2;
  }
}

export function centsToAmount(amountCents: number, currency: string): number {
  return amountCents / 10 ** currencyDigits(currency);
}

export interface MoneyOptions {
  /** `₹1.2L` style for column totals and charts. */
  compact?: boolean;
  /** Always show minor units, even for whole amounts. Default: only when non-zero. */
  showCents?: boolean;
  locale?: string;
}

export function formatMoney(
  amountCents: number,
  currency: string = "INR",
  { compact = false, showCents = false, locale = DEFAULT_LOCALE }: MoneyOptions = {},
): string {
  const code = (currency || "INR").toUpperCase();
  const digits = currencyDigits(code);
  const amount = amountCents / 10 ** digits;
  if (compact) {
    return numberFormat(locale, {
      style: "currency",
      currency: code,
      notation: "compact",
      maximumFractionDigits: Math.abs(amount) < 1000 ? 0 : 1,
    }).format(amount);
  }
  const whole = Number.isInteger(amount);
  return numberFormat(locale, {
    style: "currency",
    currency: code,
    minimumFractionDigits: showCents || !whole ? digits : 0,
    maximumFractionDigits: digits,
  }).format(amount);
}

/**
 * Parses what a person types into a money field ("12,500", "₹1,200.50") into
 * integer minor units. Returns null when the input isn't a number.
 */
export function parseMoneyToCents(input: string, currency: string = "INR"): number | null {
  const cleaned = input.replace(/[^\d.,-]/g, "").replace(/,/g, "");
  if (cleaned === "" || cleaned === "-" || cleaned === ".") return null;
  if (!/^-?\d*(\.\d*)?$/.test(cleaned)) return null;
  const value = Number(cleaned);
  if (!Number.isFinite(value)) return null;
  return Math.round(value * 10 ** currencyDigits(currency));
}

/** Formats minor units as a plain editable number ("12500.5" → "12500.50"). */
export function centsToInput(amountCents: number, currency: string = "INR"): string {
  const digits = currencyDigits(currency);
  return (amountCents / 10 ** digits).toFixed(digits).replace(/\.0+$/, "");
}

export function formatNumber(value: number, locale = DEFAULT_LOCALE): string {
  return numberFormat(locale, {}).format(value);
}

export function formatPercent(value: number, locale = DEFAULT_LOCALE): string {
  return numberFormat(locale, { style: "percent", maximumFractionDigits: 0 }).format(value / 100);
}

const dateFormat = new Intl.DateTimeFormat(DEFAULT_LOCALE, { month: "short", day: "numeric", year: "numeric" });
const shortDateFormat = new Intl.DateTimeFormat(DEFAULT_LOCALE, { month: "short", day: "numeric" });
const dateTimeFormat = new Intl.DateTimeFormat(DEFAULT_LOCALE, {
  month: "short",
  day: "numeric",
  hour: "numeric",
  minute: "2-digit",
});
const timeFormat = new Intl.DateTimeFormat(DEFAULT_LOCALE, { hour: "numeric", minute: "2-digit" });

/** Parses `YYYY-MM-DD` as a local calendar date (not UTC midnight). */
export function parseDateOnly(value: string): Date {
  const [y = Number.NaN, m = 1, d = 1] = value.split("-").map(Number);
  return new Date(y, m - 1, d);
}

export function formatDate(value: string | null | undefined): string {
  if (!value) return "";
  const date = /^\d{4}-\d{2}-\d{2}$/.test(value) ? parseDateOnly(value) : new Date(value);
  return date.getFullYear() === new Date().getFullYear() ? shortDateFormat.format(date) : dateFormat.format(date);
}

export function formatDateLong(value: string | null | undefined): string {
  if (!value) return "";
  const date = /^\d{4}-\d{2}-\d{2}$/.test(value) ? parseDateOnly(value) : new Date(value);
  return dateFormat.format(date);
}

export function formatDateTime(value: string | null | undefined): string {
  if (!value) return "";
  return dateTimeFormat.format(new Date(value));
}

export function formatTime(value: string | null | undefined): string {
  if (!value) return "";
  return timeFormat.format(new Date(value));
}

const relative = new Intl.RelativeTimeFormat(DEFAULT_LOCALE, { numeric: "auto" });

export function formatRelative(value: string | null | undefined, now: Date = new Date()): string {
  if (!value) return "";
  const diffSeconds = (new Date(value).getTime() - now.getTime()) / 1000;
  const abs = Math.abs(diffSeconds);
  if (abs < 45) return "just now";
  if (abs < 3600) return relative.format(Math.round(diffSeconds / 60), "minute");
  if (abs < 86400) return relative.format(Math.round(diffSeconds / 3600), "hour");
  if (abs < 86400 * 7) return relative.format(Math.round(diffSeconds / 86400), "day");
  return formatDate(value);
}

/** Start of the current UTC day (the API's `due` filter works in UTC days). */
export function startOfUtcDay(now: Date = new Date()): number {
  return Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate());
}

/**
 * Bucket for an open task's due date, matching the API's `due` filter:
 * overdue = due before today, today = due during today, upcoming = after
 * today, with "today" being the current UTC day.
 */
export function dueBucket(dueAt: string | null, now: Date = new Date()): "overdue" | "today" | "upcoming" | null {
  if (!dueAt) return null;
  const due = new Date(dueAt).getTime();
  const start = startOfUtcDay(now);
  if (due < start) return "overdue";
  if (due < start + 86_400_000) return "today";
  return "upcoming";
}

export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  const first = parts[0];
  const last = parts[parts.length - 1];
  if (!first || !last) return "?";
  if (parts.length === 1) return first.slice(0, 2).toUpperCase();
  return (first.charAt(0) + last.charAt(0)).toUpperCase();
}

/** Today's date as `YYYY-MM-DD` in local time. */
export function todayIso(now: Date = new Date()): string {
  const y = now.getFullYear();
  const m = String(now.getMonth() + 1).padStart(2, "0");
  const d = String(now.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}
