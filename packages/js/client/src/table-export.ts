/**
 * Exporting a table. Navree UI's export is a link to the server with the
 * table's state in the address, and the server writes the whole result as a
 * file: `exportHref` is that link. `toCsv` writes the same file from rows you
 * hold, with Navree UI's rules: RFC 4180 quoting, a byte-order mark and CRLF so
 * a spreadsheet opens it, and no cell that a spreadsheet would run as a formula.
 */
import { serializeListQuery, type ListQuery } from "./list-query.js";

/** `query` as an export asks for it: the search, filters and sort, and no page (an export is every row). */
export function exportQuery(query: ListQuery): ListQuery {
  const { page: _page, per_page: _perPage, cursor: _cursor, before: _before, limit: _limit, ...rest } = query;
  return rest;
}

/** `url` with the table's state as its query: the address an Export link points to. */
export function exportHref(url: string, query: ListQuery): string {
  const params = serializeListQuery(exportQuery(query)).toString();
  if (params === "") return url;
  const hash = url.indexOf("#");
  const base = hash < 0 ? url : url.slice(0, hash);
  const tail = hash < 0 ? "" : url.slice(hash);
  return `${base}${base.includes("?") ? "&" : "?"}${params}${tail}`;
}

/** A negative number is a number; `-` followed by anything else may be a formula. */
const PLAIN_NEGATIVE = /^-(?:inf(?:inity)?|nan|(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?)$/i;

/**
 * One CSV field. A value that starts with `=`, `+`, `@`, a tab or a carriage return, or with `-` and is not
 * a plain negative number, gets a leading apostrophe so a spreadsheet shows text rather than running it.
 */
export function csvField(value: string): string {
  const dangerous = /^[=+@\t\r]/.test(value) || (value.startsWith("-") && !PLAIN_NEGATIVE.test(value));
  const quote = dangerous || /[,"\n\r]/.test(value);
  if (!quote) return value;
  return `"${dangerous ? "'" : ""}${value.replaceAll('"', '""')}"`;
}

/** The file's first character: a byte-order mark, so a spreadsheet reads UTF-8 (names in Hindi and Punjabi stay as written). */
export const CSV_BOM = "﻿";

/** A CSV file: the head, then each row, CRLF between lines. */
export function toCsv(head: readonly string[], rows: readonly (readonly string[])[], { bom = true }: { bom?: boolean } = {}): string {
  const lines = [head, ...rows].map((cells) => cells.map(csvField).join(","));
  return `${bom ? CSV_BOM : ""}${lines.join("\r\n")}\r\n`;
}
