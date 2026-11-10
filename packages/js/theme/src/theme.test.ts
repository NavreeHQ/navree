import { existsSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { compile } from "@tailwindcss/node";
import { describe, expect, it } from "vitest";
import { colors, contrastRatio, fontFamilies, semanticColors, themeColor } from "./index.js";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const css = (name: string) => readFileSync(join(root, "css", name), "utf8");

/** `--color-<name>: #hex` declarations in a block of CSS. */
function colorDeclarations(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [, name, value] of text.matchAll(/--color-([a-z0-9-]+):\s*(#[0-9a-f]{6});/g)) out[name!] = value!;
  return out;
}

describe("the tokens", () => {
  it("are the same in theme.css, tokens.css and the JavaScript export", () => {
    const themeBlock = css("theme.css").split("@theme inline")[0]!;
    expect(colorDeclarations(themeBlock)).toEqual(colors);
    expect(colorDeclarations(css("tokens.css"))).toEqual(colors);
    expect(themeColor).toBe("#040605");
  });

  // The website's stylesheet is beside the theme only when the two are checked out together.
  const siteCss = join(root, "..", "..", "..", "apps", "website", "src", "app", "globals.css");
  it.skipIf(!existsSync(siteCss))("are navree.com's values", () => {
    const site = readFileSync(siteCss, "utf8");
    const siteTheme = colorDeclarations(site.split("@theme inline")[0]!);
    expect(siteTheme).toEqual(colors);
  });

  it("meet WCAG AA: every text step on every ink step, and text on the accent", () => {
    for (const text of ["fg", "fg-1", "fg-2", "fg-3"] as const) {
      for (const ink of ["ink-0", "ink-1", "ink-2", "ink-3"] as const) {
        expect(contrastRatio(colors[text], colors[ink]), `${text} on ${ink}`).toBeGreaterThanOrEqual(4.5);
      }
    }
    expect(contrastRatio(colors["accent-ink"], colors.accent)).toBeGreaterThanOrEqual(4.5);
    for (const signal of ["accent", "amber", "danger"] as const) {
      expect(contrastRatio(colors[signal], colors["ink-1"]), signal).toBeGreaterThanOrEqual(4.5);
    }
    // fg-3 on ink-3 is the faintest text.
    expect(contrastRatio(colors["fg-3"], colors["ink-3"])).toBeCloseTo(5.4, 1);
  });

  it("computes contrast like WCAG", () => {
    expect(contrastRatio("#000000", "#ffffff")).toBeCloseTo(21, 5);
    expect(contrastRatio("#ffffff", "#ffffff")).toBeCloseTo(1, 5);
  });
});

describe("the shadcn preset", () => {
  const registry = JSON.parse(readFileSync(join(root, "registry", "navree.json"), "utf8")) as {
    type: string;
    cssVars: { light: Record<string, string>; dark: Record<string, string> };
  };

  it("is a registry theme with the same colors in light and dark (the theme is dark only)", () => {
    expect(registry.type).toBe("registry:theme");
    expect(registry.cssVars.dark).toEqual(semanticColors);
    expect(registry.cssVars.light).toEqual(registry.cssVars.dark);
  });

  it("maps every semantic color theme.css defines", () => {
    const rootBlock = css("theme.css").split(":root {")[1]!.split("}")[0]!;
    const names = [...rootBlock.matchAll(/^\s*--([a-z0-9-]+):/gm)].map(([, name]) => name!);
    for (const name of names.filter((n) => n !== "radius" && n !== "header-h")) {
      expect(Object.keys(semanticColors), name).toContain(name);
    }
  });
});

describe("Tailwind CSS 4", () => {
  async function build(candidates: string[]): Promise<string> {
    const compiler = await compile('@import "tailwindcss";\n@import "./theme.css";\n', {
      base: join(root, "css"),
      onDependency: () => undefined,
    });
    return compiler.build(candidates);
  }

  it("generates the token utilities", async () => {
    const out = await build(["bg-ink-1", "text-fg-3", "border-line-2", "text-accent", "bg-amber/10"]);
    expect(out).toContain("--color-ink-1: #0a0e0c");
    expect(out).toMatch(/\.bg-ink-1\s*\{\s*background-color: var\(--color-ink-1\)/);
    expect(out).toMatch(/\.text-fg-3\s*\{\s*color: var\(--color-fg-3\)/);
    expect(out).toContain(".border-line-2");
    expect(out).toContain(".bg-amber\\/10");
  });

  it("maps shadcn's semantic utilities onto the tokens", async () => {
    const out = await build(["bg-primary", "text-primary-foreground", "bg-sidebar-accent", "border-border", "ring-ring", "bg-chart-5"]);
    expect(out).toMatch(/\.bg-primary\s*\{\s*background-color: var\(--primary\)/);
    expect(out).toContain("--primary: var(--color-accent)");
    expect(out).toContain(".bg-sidebar-accent");
    expect(out).toContain(".bg-chart-5");
  });

  it("pins the corners to 4px controls and 6px surfaces", async () => {
    const out = await build(["rounded-sm", "rounded-md", "rounded-lg", "rounded-xl", "rounded-2xl"]);
    expect(out).toMatch(/\.rounded-md\s*\{\s*border-radius: 4px/);
    expect(out).toMatch(/\.rounded-lg\s*\{\s*border-radius: 4px/);
    expect(out).toMatch(/\.rounded-xl\s*\{\s*border-radius: 6px/);
    expect(out).toMatch(/\.rounded-2xl\s*\{\s*border-radius: 6px/);
  });

  it("has the type scale, the fonts and the app utilities", async () => {
    const out = await build(["text-display-lg", "text-title", "font-display", "font-mono", "figures", "surface", "th-label", "eyebrow", "scroll-thin", "animate-shimmer"]);
    expect(out).toContain(".text-display-lg");
    expect(out).toMatch(/\.font-display\s*\{[^}]*var\(--font-mona\)/);
    expect(out).toMatch(/\.font-mono\s*\{[^}]*var\(--font-monaspace\)/);
    expect(out).toMatch(/\.figures\s*\{\s*font-variant-numeric: tabular-nums lining-nums/);
    expect(out).toMatch(/\.surface\s*\{[^}]*background: var\(--color-ink-1\)/);
    expect(out).toContain("@keyframes shimmer");
  });

  it("applies `dark:` under .dark, and keeps rtl: and ltr: for right-to-left layouts", async () => {
    const out = await build(["dark:bg-ink-2", "rtl:text-right", "ltr:ml-2"]);
    expect(out).toMatch(/\.dark\\:bg-ink-2\s*\{\s*&:where\(\.dark, \.dark \*\)|:where\(\.dark, \.dark \*\)/);
    expect(out).toContain('[dir="rtl"]');
    expect(out).toContain('[dir="ltr"]');
  });
});

describe("fonts.css", () => {
  it("imports @fontsource files that exist and register the families the theme names", () => {
    const require = createRequire(join(root, "package.json"));
    const imports = [...css("fonts.css").matchAll(/@import "([^"]+)";/g)].map(([, spec]) => spec!);
    expect(imports).toHaveLength(5);
    const families = new Set<string>();
    for (const spec of imports) {
      const file = require.resolve(spec);
      for (const [, family] of readFileSync(file, "utf8").matchAll(/font-family: '([^']+)'/g)) families.add(family!);
    }
    expect([...families].sort()).toEqual(Object.values(fontFamilies).sort());
    for (const family of Object.values(fontFamilies)) expect(css("fonts.css")).toContain(`"${family}"`);
  });
});
