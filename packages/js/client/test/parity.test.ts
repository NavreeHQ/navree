import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import * as client from "../src/index.js";

/**
 * The parity contract between `@navree/next` and `@navree/nuxt`: the names both packages export
 * for the same job. A name moves into `shared` only when both packages offer it; what stays in a
 * list below is framework work (React components and hooks, Vue composables, Nitro handlers).
 */
const packages = join(dirname(fileURLToPath(import.meta.url)), "..", "..");

/** The names an index file exports, as the importer sees them (`export { a as b }` is `b`). */
function exportedNames(file: string): Set<string> {
  const text = readFileSync(join(packages, file), "utf8");
  const names = new Set<string>();
  for (const [, body] of text.matchAll(/export\s*(?:type\s*)?\{([^}]*)\}/g)) {
    for (const part of body!.split(",")) {
      const name = part
        .trim()
        .replace(/^type\s+/, "")
        .split(/\s+as\s+/)
        .pop();
      if (name) names.add(name);
    }
  }
  return names;
}

const next = exportedNames("next/src/client/index.ts");
const nuxt = exportedNames("nuxt/src/vue/index.ts");

/** Helpers, components and types both packages export under one name. */
const shared = [
  // Tables
  "NavreeDataTable",
  "NavreePagination",
  "useDataTable",
  "defaultLabels",
  "DataTableBulk",
  "DataTableBulkAction",
  "DataTableLabels",
  "NavreeColumnDef",
  "NavreeColumnMeta",
  "NavreeTable",
  "NavreeTableFeatures",
  "UseDataTableOptions",
  // The list query in the address
  "useListQuery",
  "mergeListQuery",
  "isListParam",
  "ListQueryState",
  // Operations
  "useOperation",
  "UseOperationOptions",
  "UseOperationResult",
  // Forms
  "applyFieldErrors",
  // Locale and errors
  "direction",
  "errorKey",
  "errorMessage",
  "isRtl",
  "localeAttributes",
  "translateError",
  "Translate",
  // The workspace cookie
  "WORKSPACE_COOKIE",
];

describe("@navree/next and @navree/nuxt agree on names", () => {
  it.each(shared)("both export %s", (name) => {
    expect(next.has(name), `@navree/next lacks ${name}`).toBe(true);
    expect(nuxt.has(name), `@navree/nuxt lacks ${name}`).toBe(true);
  });

  it("share the helpers that need no framework through @navree/client", () => {
    for (const name of ["direction", "errorKey", "errorMessage", "isRtl", "localeAttributes", "translateError", "isListParam"]) {
      expect(client, name).toHaveProperty(name);
    }
  });

  it("share the rules of a table's search, filters, selection and export through @navree/client", () => {
    for (const name of [
      "readFilter",
      "writeFilter",
      "withSearch",
      "filterKey",
      "activeFilters",
      "filterControl",
      "describeSelection",
      "reconcileSelection",
      "setSelected",
      "bulkRequest",
      "bulkParams",
      "exportHref",
      "toCsv",
      "defaultTableLabels",
    ]) {
      expect(client, name).toHaveProperty(name);
    }
  });

  it("differ only in what the framework forces", () => {
    const onlyNext = [...next].filter((name) => !nuxt.has(name)).sort();
    const onlyNuxt = [...nuxt].filter((name) => !next.has(name)).sort();
    expect(onlyNext).toEqual([
      "DataTable",
      "DataTableProps",
      "Pagination",
      "PaginationProps",
      "WorkspaceProvider",
      "WorkspaceProviderProps",
      // The first name of mergeListQuery, kept for code written against it.
      "mergeListParams",
      "readWorkspaceCookie",
      "useWorkspace",
      "writeWorkspaceCookie",
    ].sort());
    expect(onlyNuxt).toEqual(["AuthStatus", "ApplyFieldErrorsOptions", "NavreeAuth", "NavreeAuthOptions", "SignInResult", "Workspace", "createNavreeAuth", "createWorkspace"].sort());
  });
});
