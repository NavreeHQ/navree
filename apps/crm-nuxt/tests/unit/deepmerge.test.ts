// @vitest-environment node
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { describe, expect, it } from "vitest";

/**
 * Nitro's bundler loads `deepmerge` (nuxt -> @nuxt/nitro-server -> nitropack -> @rollup/plugin-node-resolve ->
 * deepmerge). The overrides in pnpm-workspace.yaml point that name at @navree/deepmerge (packages/js/deepmerge),
 * which skips the `__proto__`, `constructor` and `prototype` keys. These tests load the installed copy the way
 * Node does at run time and fail if it is not Navree's, or if it copies one of those keys.
 */
type Merge = (target: unknown, source: unknown) => Record<string, unknown>;

function resolveDeepmerge(): { merge: Merge; packageJson: string } {
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
  return { merge: require("deepmerge") as Merge, packageJson: require.resolve("deepmerge/package.json") };
}

const { merge, packageJson: deepmergePackageJson } = resolveDeepmerge();
const attack = '{"__proto__": {"isAdmin": true}}';

describe("deepmerge under Nitro's bundler", () => {
  it("is @navree/deepmerge", () => {
    const manifest = JSON.parse(readFileSync(deepmergePackageJson, "utf8")) as { name: string };
    expect(manifest.name).toBe("@navree/deepmerge");
  });

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
