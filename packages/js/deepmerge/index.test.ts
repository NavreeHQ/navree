import { describe, expect, it } from "vitest";
import merge from "./index.cjs";

const poisoned = '{"__proto__": {"isAdmin": true}}';

describe("merging", () => {
  it("merges objects deeply and concatenates arrays", () => {
    expect(merge({ a: { b: 1 }, list: [1] }, { a: { c: 2 }, list: [2], d: null })).toEqual({
      a: { b: 1, c: 2 },
      list: [1, 2],
      d: null,
    });
  });

  it("takes the source's value where the types differ or the value is not mergeable", () => {
    expect(merge({ a: [1], b: { x: 1 }, c: 1 }, { a: { y: 1 }, b: [2], c: "s" })).toEqual({
      a: { y: 1 },
      b: [2],
      c: "s",
    });
  });

  it("leaves both inputs untouched and returns copies of what it merges", () => {
    const target = { a: { b: [1] } };
    const source = { a: { c: { d: 1 } } };
    const merged = merge<Record<string, any>>(target, source);
    expect(target).toEqual({ a: { b: [1] } });
    expect(source).toEqual({ a: { c: { d: 1 } } });
    expect(merged.a).not.toBe(target.a);
    expect(merged.a.c).not.toBe(source.a.c);
    expect(merged.a.b).not.toBe(target.a.b);
  });

  it("keeps dates, regular expressions and React elements as they are", () => {
    const date = new Date(0);
    const pattern = /a/g;
    const element = { $$typeof: Symbol.for("react.element"), props: {} };
    const merged = merge<Record<string, any>>({}, { date, pattern, element });
    expect(merged.date).toBe(date);
    expect(merged.pattern).toBe(pattern);
    expect(merged.element).toBe(element);
  });

  it("copies enumerable symbol keys and skips non-enumerable ones", () => {
    const key = Symbol("k");
    const source: Record<string | symbol, unknown> = { [key]: 1 };
    Object.defineProperty(source, "hidden", { value: 2, enumerable: false });
    const merged = merge<Record<string | symbol, unknown>>({}, source);
    expect(merged[key]).toBe(1);
    expect("hidden" in merged).toBe(false);
  });

  it("does not merge into a property the target only inherits", () => {
    const merged = merge<Record<string, unknown>>({}, { toString: { a: 1 }, valueOf: 5 });
    expect(Object.keys(merged)).toEqual([]);
  });
});

describe("options", () => {
  it("clone: false keeps references to what it does not merge", () => {
    const inner = { n: 1 };
    const merged = merge<Record<string, any>>({}, { inner }, { clone: false });
    expect(merged.inner).toBe(inner);
  });

  it("arrayMerge decides how arrays combine and receives the clone helper", () => {
    const overwrite = merge({ list: [1, 2] }, { list: [3] }, { arrayMerge: (_target, source) => source });
    expect(overwrite).toEqual({ list: [3] });
    const byIndex = merge<Record<string, any>>(
      { list: [{ a: 1 }] },
      { list: [{ b: 2 }] },
      {
        arrayMerge: (target, source, options) =>
          target.map((item, i) => options!.cloneUnlessOtherwiseSpecified(source[i] ?? item, options as any)),
      },
    );
    expect(byIndex).toEqual({ list: [{ b: 2 }] });
  });

  it("customMerge replaces the merge for one key", () => {
    const merged = merge(
      { a: { x: 1 }, b: { x: 1 } },
      { a: { y: 2 }, b: { y: 2 } },
      { customMerge: (key) => (key === "a" ? (x, y) => [x, y] : undefined) },
    );
    expect(merged).toEqual({ a: [{ x: 1 }, { y: 2 }], b: { x: 1, y: 2 } });
  });

  it("isMergeableObject limits what is merged", () => {
    class Box {
      constructor(public n: number) {}
    }
    const box = new Box(1);
    const merged = merge<Record<string, any>>(
      { a: { x: 1 } },
      { a: { y: 2 }, box },
      { isMergeableObject: (value) => Object.getPrototypeOf(value) === Object.prototype },
    );
    expect(merged.a).toEqual({ x: 1, y: 2 });
    expect(merged.box).toBe(box);
  });
});

describe("all", () => {
  it("merges a list left to right", () => {
    expect(merge.all([{ a: 1 }, { b: { c: 1 } }, { b: { d: 2 } }])).toEqual({ a: 1, b: { c: 1, d: 2 } });
    expect(merge.all([])).toEqual({});
  });

  it("refuses anything but an array", () => {
    expect(() => merge.all({} as unknown as object[])).toThrow("first argument should be an array");
  });
});

describe("prototype poisoning", () => {
  it("does not take a prototype from the source when the target has a null prototype", () => {
    const merged = merge<Record<string, unknown>>(Object.create(null), JSON.parse(poisoned));
    expect(Object.getPrototypeOf(merged)).toBe(Object.prototype);
    expect(merged.isAdmin).toBeUndefined();
  });

  it("does not take a prototype from the source under a null placeholder", () => {
    const merged = merge<Record<string, any>>({ session: null }, JSON.parse(`{"session": ${poisoned}}`));
    expect(Object.getPrototypeOf(merged.session)).toBe(Object.prototype);
    expect(merged.session.isAdmin).toBeUndefined();
  });

  it("does not take a prototype from the source under a primitive placeholder", () => {
    const merged = merge<Record<string, any>>({ session: 1 }, JSON.parse(`{"session": ${poisoned}}`));
    expect(Object.getPrototypeOf(merged.session)).toBe(Object.prototype);
    expect(merged.session.isAdmin).toBeUndefined();
  });

  it("does not copy a target's own __proto__ key into the result's prototype", () => {
    const merged = merge<Record<string, unknown>>(JSON.parse(poisoned), { name: "x" });
    expect(Object.getPrototypeOf(merged)).toBe(Object.prototype);
    expect(merged.isAdmin).toBeUndefined();
    expect(merged.name).toBe("x");
  });

  it("skips constructor and prototype keys", () => {
    const merged = merge<Record<string, unknown>>(
      {},
      JSON.parse('{"constructor": {"prototype": {"isAdmin": true}}, "prototype": {"isAdmin": true}}'),
    );
    expect(Object.keys(merged)).toEqual([]);
    expect(merged.constructor).toBe(Object);
    expect(({} as Record<string, unknown>).isAdmin).toBeUndefined();
  });

  it("never changes Object.prototype, nested, in arrays or through all()", () => {
    merge({}, JSON.parse(`{"a": {"b": ${poisoned}}}`));
    merge.all([JSON.parse(poisoned), JSON.parse('{"constructor": {"prototype": {"isAdmin": true}}}')]);
    merge({ list: [{}] }, JSON.parse(`{"list": [${poisoned}]}`));
    expect(({} as Record<string, unknown>).isAdmin).toBeUndefined();
    expect(Object.prototype.hasOwnProperty.call(Object.prototype, "isAdmin")).toBe(false);
  });
});

describe("depth", () => {
  function nest(levels: number): Record<string, unknown> {
    const root: Record<string, unknown> = {};
    let node = root;
    for (let i = 1; i < levels; i++) {
      const child: Record<string, unknown> = {};
      node.n = child;
      node = child;
    }
    return root;
  }

  it("merges as deep as maxDepth", () => {
    expect(merge.maxDepth).toBe(1000);
    expect(() => merge({}, nest(merge.maxDepth))).not.toThrow();
  });

  it("refuses a deeper input with a RangeError", () => {
    expect(() => merge({}, nest(merge.maxDepth + 1))).toThrow(RangeError);
  });

  it("refuses a self-referencing input", () => {
    const loop: Record<string, unknown> = {};
    loop.self = loop;
    expect(() => merge({}, loop)).toThrow(RangeError);
  });

  it("merges again after a refusal", () => {
    expect(() => merge({}, nest(merge.maxDepth + 1))).toThrow(RangeError);
    expect(merge({ a: { b: 1 } }, { a: { c: 2 } })).toEqual({ a: { b: 1, c: 2 } });
  });
});
