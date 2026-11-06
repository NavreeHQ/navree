import { describe, expect, it, vi } from "vitest";
import { appearanceScript, applyAppearance, parseAppearance, resolveMode, type AppearanceRoot } from "./appearance.js";
import { themeFromBuilderCss } from "./custom-theme.js";

function fakeRoot() {
  const attributes = new Map<string, string>();
  const classes = new Set<string>();
  const root = {
    setAttribute: (name: string, value: string) => void attributes.set(name, value),
    classList: {
      toggle: (name: string, force?: boolean) => {
        if (force === undefined ? !classes.has(name) : force) classes.add(name);
        else classes.delete(name);
        return classes.has(name);
      },
    },
  } as unknown as AppearanceRoot;
  return { root, attributes, classes };
}

function fakeMedia(initial: boolean) {
  let listener: (() => void) | undefined;
  const query = {
    matches: initial,
    addEventListener: vi.fn((_: string, fn: () => void) => void (listener = fn)),
    removeEventListener: vi.fn(() => void (listener = undefined)),
  };
  return {
    matchMedia: () => query as never,
    query,
    change: (matches: boolean) => {
      query.matches = matches;
      listener?.();
    },
    listening: () => listener !== undefined,
  };
}

describe("resolveMode", () => {
  it("follows the system only in system mode", () => {
    expect(resolveMode("light", true)).toBe("light");
    expect(resolveMode("dark", false)).toBe("dark");
    expect(resolveMode("system", true)).toBe("dark");
    expect(resolveMode("system", false)).toBe("light");
  });
});

describe("applyAppearance", () => {
  it("sets the theme, the mode and the dark class", () => {
    const { root, attributes, classes } = fakeRoot();
    applyAppearance(root, { theme: "ocean", mode: "light" }, fakeMedia(true).matchMedia);
    expect(Object.fromEntries(attributes)).toEqual({ "data-theme": "ocean", "data-mode": "light" });
    expect(classes.has("dark")).toBe(false);

    applyAppearance(root, { theme: "rose", mode: "dark" }, fakeMedia(false).matchMedia);
    expect(attributes.get("data-theme")).toBe("rose");
    expect(classes.has("dark")).toBe(true);
  });

  it("is the Navree theme in dark mode when it is given nothing", () => {
    const { root, attributes, classes } = fakeRoot();
    applyAppearance(root, undefined, fakeMedia(false).matchMedia);
    expect(Object.fromEntries(attributes)).toEqual({ "data-theme": "navree", "data-mode": "dark" });
    expect(classes.has("dark")).toBe(true);
  });

  it("in system mode follows the system until it is stopped", () => {
    const { root, classes } = fakeRoot();
    const media = fakeMedia(true);
    const stop = applyAppearance(root, { mode: "system" }, media.matchMedia);
    expect(classes.has("dark")).toBe(true);

    media.change(false);
    expect(classes.has("dark")).toBe(false);
    media.change(true);
    expect(classes.has("dark")).toBe(true);

    stop();
    expect(media.listening()).toBe(false);
  });

  it("does not listen to the system when the mode is fixed", () => {
    const media = fakeMedia(true);
    applyAppearance(fakeRoot().root, { mode: "light" }, media.matchMedia);
    expect(media.query.addEventListener).not.toHaveBeenCalled();
  });

  it("works where there is no matchMedia: dark stays dark", () => {
    const { root, classes } = fakeRoot();
    const stop = applyAppearance(root, { mode: "system" }, undefined);
    expect(classes.has("dark")).toBe(true);
    expect(stop()).toBeUndefined();
  });
});

describe("parseAppearance", () => {
  it("reads a stored choice and drops anything that is not one", () => {
    expect(parseAppearance('{"theme":"sunset","mode":"system"}')).toEqual({ theme: "sunset", mode: "system" });
    expect(parseAppearance('{"theme":"Bad Theme","mode":"neon"}')).toEqual({});
    expect(parseAppearance('{"theme":"a\\"><script>"}')).toEqual({});
    expect(parseAppearance("not json")).toEqual({});
    expect(parseAppearance("null")).toEqual({});
    expect(parseAppearance(undefined)).toEqual({});
  });
});

describe("appearanceScript", () => {
  function run(script: string, stored: string | null, prefersDark: boolean) {
    const { root, attributes, classes } = fakeRoot();
    const document = { documentElement: root };
    const localStorage = { getItem: () => stored };
    const matchMedia = () => ({ matches: prefersDark });
    new Function("document", "localStorage", "matchMedia", script)(document, localStorage, matchMedia);
    return { theme: attributes.get("data-theme"), mode: attributes.get("data-mode"), dark: classes.has("dark") };
  }

  it("applies the stored choice before the first paint", () => {
    const script = appearanceScript();
    expect(run(script, '{"theme":"violet","mode":"light"}', true)).toEqual({ theme: "violet", mode: "light", dark: false });
    expect(run(script, '{"theme":"violet","mode":"system"}', true)).toEqual({ theme: "violet", mode: "system", dark: true });
    expect(run(script, '{"theme":"violet","mode":"system"}', false)).toEqual({ theme: "violet", mode: "system", dark: false });
  });

  it("falls back to the defaults for nothing, nonsense, or a name that is not a mode", () => {
    const script = appearanceScript({ defaults: { theme: "ocean", mode: "system" } });
    expect(run(script, null, false)).toEqual({ theme: "ocean", mode: "system", dark: false });
    expect(run(script, "{oops", true)).toEqual({ theme: "ocean", mode: "system", dark: true });
    expect(run(script, '{"mode":"constructor","theme":"x y"}', true)).toEqual({ theme: "ocean", mode: "system", dark: true });
  });

  it("reads the key it is given, and quotes everything it embeds", () => {
    const script = appearanceScript({ storageKey: 'a"b', defaults: { theme: 'x"y' } });
    expect(script).toContain('"a\\"b"');
    expect(script).toContain('"x\\"y"');

    const hostile = appearanceScript({ storageKey: "</script><script>alert(1)</script>" });
    expect(hostile).not.toContain("</script");
    expect(hostile).toContain("\\u003c/script");
  });
});

describe("themeFromBuilderCss", () => {
  const builder =
    ':where([data-theme=custom]){--ink-0:light-dark(#ffffff,#07080c);--ink-1:light-dark(#f6f7fb,#0e1016);--line-3:light-dark(#aaaaaa,#444444);' +
    "--fg:light-dark(#0e1016,#eceff7);--accent:light-dark(#d6481f,#ff9a6e);--accent-ink:#ffffff;--radius:6px;--ui-tick:url(\"data:image/svg+xml,%3Csvg%3E\")}\n" +
    ":where([data-theme=custom][data-mode=light]){--ui-tick:url(\"data:image/svg+xml,%3Csvg%3E\")}\n";

  it("renames the colours theme.css names to the Tailwind colours, under the theme's own attribute", () => {
    expect(themeFromBuilderCss(builder)).toBe(
      [
        '[data-theme="custom"] {',
        "  --color-ink-0: light-dark(#ffffff, #07080c);",
        "  --color-ink-1: light-dark(#f6f7fb, #0e1016);",
        "  --color-fg: light-dark(#0e1016, #eceff7);",
        "  --color-accent: light-dark(#d6481f, #ff9a6e);",
        "  --color-accent-ink: #ffffff;",
        "}",
        "",
      ].join("\n"),
    );
  });

  it("refuses CSS that has no theme", () => {
    expect(() => themeFromBuilderCss("body{color:red}")).toThrow(/no theme block/);
  });
});
