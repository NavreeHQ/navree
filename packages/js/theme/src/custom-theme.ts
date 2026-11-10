/**
 * A theme made in Navree UI's theme builder, for an app that uses `@navree/theme`. The builder
 * writes its colours as `--ink-0`, `--fg` and so on under `:where([data-theme=key])`; this app's
 * Tailwind names them `--color-ink-0`. `themeFromBuilderCss` renames them, so the builder's
 * output reaches a Next.js or Nuxt app.
 */

/** The colours `theme.css` names as Tailwind colours. */
const COLOURS = [
  "ink-0",
  "ink-1",
  "ink-2",
  "ink-3",
  "line",
  "line-2",
  "fg",
  "fg-1",
  "fg-2",
  "fg-3",
  "accent",
  "accent-ink",
  "amber",
  "danger",
  "bar",
] as const;

const VALUE = "(?:light-dark\\(#[0-9a-fA-F]{6},#[0-9a-fA-F]{6}\\)|#[0-9a-fA-F]{6})";

/**
 * The stylesheet for a theme from the builder's CSS (its "Copy CSS" output, or the stylesheet
 * Navree UI serves for it): `[data-theme="key"]` with `--color-*` colours, for `themes.css`'s
 * attributes. Add it to your CSS after `themes.css`. Throws when there is no theme in `css`.
 */
export function themeFromBuilderCss(css: string): string {
  const blocks: string[] = [];
  // The theme's own block only: `[data-mode=...]` blocks for the same key carry images, not colours.
  for (const [, key, body] of css.matchAll(/:where\(\[data-theme=([A-Za-z0-9_-]+)\]\)\{([^}]*)\}/g)) {
    const lines: string[] = [];
    for (const name of COLOURS) {
      const found = new RegExp(`(?:^|;)--${name}:(${VALUE})(?=;|$)`).exec(body ?? "");
      if (found) lines.push(`  --color-${name}: ${found[1]!.replace(",", ", ")};`);
    }
    if (lines.length > 0) blocks.push(`[data-theme="${key}"] {\n${lines.join("\n")}\n}\n`);
  }
  if (blocks.length === 0) throw new Error("the CSS has no theme block (:where([data-theme=key]){--ink-0:...})");
  return blocks.join("\n");
}
