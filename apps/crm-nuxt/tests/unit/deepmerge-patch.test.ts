import { createRequire } from "node:module";
import { describe, expect, it } from "vitest";

/**
 * deepmerge 4.3.1 is the latest release and has no fix for GHSA-gcqx-w48m-w2h4 (prototype poisoning in
 * mergeObject). Nitro's bundler loads it (nuxt -> @nuxt/nitro-server -> nitropack ->
 * @rollup/plugin-node-resolve -> deepmerge), so `patches/deepmerge@4.3.1.patch` (pnpm-workspace.yaml,
 * patchedDependencies) makes it skip the `__proto__`, `constructor` and `prototype` keys. These tests load
 * that installed copy and fail if the patch is no longer applied.
 */
type Merge = (target: unknown, source: unknown) => Record<string, unknown>;

function loadDeepmerge(): Merge {
  let require = createRequire(import.meta.url);
  for (const pkg of ["nuxt", "@nuxt/nitro-server", "nitropack", "@rollup/plugin-node-resolve"]) {
    // Each step resolves from the package before it, as Node does at run time. Some packages export
    // package.json, others only their entry point; either is inside the package.
    let entry: string | undefined;
    for (const spec of [`${pkg}/package.json`, pkg]) {
      try {
        entry = require.resolve(spec);
        break;
      } catch {
        // try the next form
      }
    }
    if (!entry) throw new Error(`cannot resolve ${pkg}`);
    require = createRequire(entry);
  }
  return require("deepmerge") as Merge;
}

const merge = loadDeepmerge();
const attack = '{"__proto__": {"isAdmin": true}}';

describe("deepmerge (patched for GHSA-gcqx-w48m-w2h4)", () => {
  it("does not take a prototype from the source when the target has a null prototype", () => {
    const merged = merge(Object.create(null), JSON.parse(attack));
    expect(Object.getPrototypeOf(merged)).toBe(Object.prototype);
    expect(merged.isAdmin).toBeUndefined();
  });

  it("does not take a prototype from the source under a null placeholder", () => {
    const merged = merge({ session: null }, JSON.parse(`{"session": ${attack}}`));
    const session = merged.session as Record<string, unknown>;
    expect(Object.getPrototypeOf(session)).toBe(Object.prototype);
    expect(session.isAdmin).toBeUndefined();
  });

  it("does not copy a target's own __proto__ key into the result's prototype", () => {
    const merged = merge(JSON.parse(attack), { name: "x" });
    expect(Object.getPrototypeOf(merged)).toBe(Object.prototype);
    expect(merged.isAdmin).toBeUndefined();
    expect(merged.name).toBe("x");
  });

  it("skips constructor and prototype keys", () => {
    const merged = merge(
      {},
      JSON.parse('{"constructor": {"prototype": {"isAdmin": true}}, "prototype": {"isAdmin": true}}'),
    );
    expect(Object.keys(merged)).toEqual([]);
    expect(merged.constructor).toBe(Object);
    expect(({} as Record<string, unknown>).isAdmin).toBeUndefined();
  });

  it("still merges everything else deeply", () => {
    expect(merge({ a: { b: 1 }, list: [1] }, { a: { c: 2 }, list: [2], d: null })).toEqual({
      a: { b: 1, c: 2 },
      list: [1, 2],
      d: null,
    });
  });
});
