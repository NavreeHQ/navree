import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { compile } from "@tailwindcss/node";
import { describe, expect, it } from "vitest";
import { COLOUR_TOKENS, CSS_TOKENS, SEMANTIC, readThemes, themesCss, themesTs } from "../scripts/themes-source.mjs";
import { colors, contrastRatio } from "./index.js";
import { colourTokens, themeIds, themes } from "./themes.js";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const ui = join(root, "..", "..", "..", "crates", "navree-ui");
const read = (path: string) => readFileSync(path, "utf8").replace(/\r\n/g, "\n");

const source = { tokensCss: read(join(ui, "styles", "tokens.css")), themeRs: read(join(ui, "src", "theme", "mod.rs")) };
const fromUi = readThemes(source);

describe("the built-in themes come from navree-ui's table", () => {
  it("are the themes navree-ui names, the default first", () => {
    expect(fromUi.map((theme) => theme.id)).toEqual(["navree", "neutral", "ocean", "violet", "sunset", "rose", "contrast"]);
    expect([...themeIds]).toEqual(fromUi.map((theme) => theme.id));
    for (const theme of fromUi) {
      expect(theme.label, theme.id).not.toBe("");
      expect(theme.description, theme.id).not.toBe("");
    }
    expect(themes.contrast.label).toBe("High contrast");
  });

  it("are written to css/themes.css and src/themes.ts: run `pnpm generate` in packages/js/theme after navree-ui's tokens change", () => {
    expect(read(join(root, "css", "themes.css"))).toBe(themesCss(fromUi));
    expect(read(join(root, "src", "themes.ts"))).toBe(themesTs(fromUi));
  });

  it("name the same colours in the data and in navree-ui", () => {
    expect([...colourTokens]).toEqual(COLOUR_TOKENS);
    for (const theme of fromUi) {
      expect(themes[theme.id as keyof typeof themes].light, theme.id).toEqual(theme.light);
      expect(themes[theme.id as keyof typeof themes].dark, theme.id).toEqual(theme.dark);
    }
  });

  it("keep the Navree theme's dark colours equal to the ones theme.css and the package's colors have", () => {
    for (const name of Object.keys(colors) as (keyof typeof colors)[]) {
      expect(themes.navree.dark[name as keyof typeof themes.navree.dark], name).toBe(colors[name]);
    }
  });
});

describe("themes.css", () => {
  const css = read(join(root, "css", "themes.css"));

  it("sets, for every theme, exactly the colours the data has where they differ from Navree", () => {
    for (const theme of fromUi) {
      const block =
        theme.id === "navree"
          ? css.split(":where(:root[data-theme], :root[data-mode], [data-theme=\"navree\"]) {")[1]!.split("}")[0]!
          : css.split(`[data-theme="${theme.id}"] {`)[1]!.split("}")[0]!;
      const declared = Object.fromEntries(
        [...block.matchAll(/--color-([a-z0-9-]+): light-dark\((#[0-9a-f]{6}), (#[0-9a-f]{6})\);/g)].map(([, name, light, dark]) => [
          name!,
          [light, dark],
        ]),
      );
      for (const name of CSS_TOKENS) {
        const own = [theme.light[name], theme.dark[name]];
        const navree = [fromUi[0]!.light[name], fromUi[0]!.dark[name]];
        if (theme.id === "navree" || own.join() !== navree.join()) expect(declared[name], `${theme.id} ${name}`).toEqual(own);
        else expect(declared[name], `${theme.id} ${name}`).toBeUndefined();
      }
    }
  });

  it("only sets colours theme.css already names, so every utility follows", () => {
    const themeCss = read(join(root, "css", "theme.css"));
    for (const name of CSS_TOKENS) expect(themeCss, name).toMatch(new RegExp(`--color-${name}: #[0-9a-f]{6};`));
  });

  it("repeats the shadcn names theme.css reads from the colours, so a part of a page can have its own theme", () => {
    const themeCss = read(join(root, "css", "theme.css"));
    const rootBlock = themeCss.split(":root {")[1]!.split("}")[0]!;
    const mapped = [...rootBlock.matchAll(/^\s*--([a-z0-9-]+): var\(--color-([a-z0-9-]+)\);/gm)].map(([, name, token]) => [name!, `var(--color-${token})`]);
    expect(SEMANTIC).toEqual(mapped);
  });

  it("works in Tailwind: the utilities read the variables a theme sets", async () => {
    const compiler = await compile('@import "tailwindcss";\n@import "./theme.css";\n', { base: join(root, "css"), onDependency: () => undefined });
    const out = compiler.build(["bg-ink-1", "text-fg-2"]);
    expect(out).toMatch(/\.bg-ink-1\s*\{\s*background-color: var\(--color-ink-1\)/);
    expect(out).toMatch(/\.text-fg-2\s*\{\s*color: var\(--color-fg-2\)/);
  });
});

describe("every theme, in both modes", () => {
  const text = ["fg", "fg-1", "fg-2"] as const;
  const surfaces = ["ink-0", "ink-1", "ink-2"] as const;

  for (const theme of fromUi) {
    for (const mode of ["light", "dark"] as const) {
      it(`${theme.id} ${mode}: body text reaches WCAG AA on the surfaces, and text on the accent too`, () => {
        const palette = theme[mode];
        for (const t of text) {
          for (const surface of surfaces) {
            expect(contrastRatio(palette[t]!, palette[surface]!), `${t} on ${surface}`).toBeGreaterThanOrEqual(4.5);
          }
        }
        expect(contrastRatio(palette["accent-ink"]!, palette.accent!)).toBeGreaterThanOrEqual(4.5);
      });
    }
  }

  it("high contrast reaches 7:1 for the main text", () => {
    for (const mode of ["light", "dark"] as const) {
      expect(contrastRatio(themes.contrast[mode].fg, themes.contrast[mode]["ink-0"])).toBeGreaterThanOrEqual(7);
    }
  });
});
