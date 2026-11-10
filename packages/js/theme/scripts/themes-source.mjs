// Reads navree-ui's theme table and writes what @navree/theme gives from it: css/themes.css and
// src/themes.ts. The table is navree-ui's generated stylesheet (styles/tokens.css, which a test in
// navree-ui holds equal to the Rust table) and the names of the built-in themes in
// src/theme/mod.rs. The test in src/themes.test.ts fails when either generated file is stale.
//
//   node scripts/generate-themes.mjs          writes both files
//   node scripts/generate-themes.mjs --check  fails when they are stale

/** The colours a theme sets, in the order the files list them. */
export const COLOUR_TOKENS = [
  "ink-0",
  "ink-1",
  "ink-2",
  "ink-3",
  "line",
  "line-2",
  "line-3",
  "fg",
  "fg-1",
  "fg-2",
  "fg-3",
  "accent",
  "accent-ink",
  "accent-text",
  "ring",
  "success",
  "amber",
  "danger",
  "info",
  "violet",
  "cyan",
  "rose",
  "bar",
];

/** The colours theme.css already names as Tailwind colours (`--color-*`): the ones themes.css sets. */
export const CSS_TOKENS = [
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
];

/** The shadcn names in theme.css's `:root` that are read from the colours. */
export const SEMANTIC = [
  ["background", "var(--color-ink-0)"],
  ["foreground", "var(--color-fg)"],
  ["card", "var(--color-ink-1)"],
  ["card-foreground", "var(--color-fg)"],
  ["popover", "var(--color-ink-1)"],
  ["popover-foreground", "var(--color-fg)"],
  ["primary", "var(--color-accent)"],
  ["primary-foreground", "var(--color-accent-ink)"],
  ["secondary", "var(--color-ink-2)"],
  ["secondary-foreground", "var(--color-fg)"],
  ["muted", "var(--color-ink-2)"],
  ["muted-foreground", "var(--color-fg-2)"],
  ["destructive", "var(--color-danger)"],
  ["warning", "var(--color-amber)"],
  ["success", "var(--color-accent)"],
  ["border", "var(--color-line)"],
  ["input", "var(--color-line-2)"],
  ["ring", "var(--color-accent)"],
  ["sidebar", "var(--color-ink-1)"],
  ["sidebar-foreground", "var(--color-fg-2)"],
  ["sidebar-primary", "var(--color-fg)"],
  ["sidebar-primary-foreground", "var(--color-ink-0)"],
  ["sidebar-accent", "var(--color-ink-2)"],
  ["sidebar-accent-foreground", "var(--color-fg)"],
  ["sidebar-border", "var(--color-line)"],
  ["sidebar-ring", "var(--color-accent)"],
  ["chart-5", "var(--color-accent)"],
];

const HEX = "#[0-9a-fA-F]{6}";

/**
 * The colours of one block of navree-ui's stylesheet: `--name:light-dark(light,dark)`, or a plain
 * value for both modes.
 * @param {string} block
 * @returns {Record<string, { light: string, dark: string }>}
 */
function colours(block) {
  const out = {};
  for (const name of COLOUR_TOKENS) {
    const pair = new RegExp(`(?:^|[;{])--${name}:light-dark\\((${HEX}),(${HEX})\\)`).exec(block);
    if (pair) {
      out[name] = { light: pair[1].toLowerCase(), dark: pair[2].toLowerCase() };
      continue;
    }
    const plain = new RegExp(`(?:^|[;{])--${name}:(${HEX})(?:;|$)`).exec(block);
    if (plain) out[name] = { light: plain[1].toLowerCase(), dark: plain[1].toLowerCase() };
  }
  return out;
}

/**
 * The text of a `const fn <name>(self) -> &'static str` of the built-in themes: variant to string.
 * @param {string} source
 * @param {string} name
 * @returns {Record<string, string>}
 */
function builtinStrings(source, name) {
  const impl = source.indexOf("impl Builtin {");
  if (impl === -1) throw new Error("navree-ui's theme/mod.rs has no built-in themes");
  const scope = source.slice(impl);
  const start = scope.indexOf(`\n    const fn ${name}(self) -> &'static str {`);
  if (start === -1) throw new Error(`navree-ui's theme/mod.rs has no \`${name}\` of the built-in themes`);
  const rest = scope.slice(start + 1);
  const next = rest.search(/\n    (?:const )?fn /);
  const body = next === -1 ? rest : rest.slice(0, next);
  const out = {};
  for (const [, variant, text] of body.matchAll(/Self::(\w+)\s*=>\s*"([^"]*)"/g)) out[variant] = text;
  return out;
}

/**
 * @typedef {{ id: string, label: string, description: string, light: Record<string, string>, dark: Record<string, string> }} Theme
 */

/**
 * Every built-in theme, with all its colours in both modes.
 * @param {{ tokensCss: string, themeRs: string }} source navree-ui's styles/tokens.css and src/theme/mod.rs
 * @returns {Theme[]}
 */
export function readThemes({ tokensCss, themeRs }) {
  const lines = tokensCss.replace(/\r\n/g, "\n").split("\n");
  const base = lines.find((line) => line.startsWith(":where(:root,[data-theme]){"));
  if (!base) throw new Error("navree-ui's tokens.css has no default theme block");
  const defaults = colours(base);
  for (const name of COLOUR_TOKENS) {
    if (!defaults[name]) throw new Error(`navree-ui's default theme has no ${name} colour`);
  }

  const ids = builtinStrings(themeRs, "id");
  const names = builtinStrings(themeRs, "name");
  const descriptions = builtinStrings(themeRs, "description");

  /** @type {Theme[]} */
  const themes = [];
  for (const [variant, id] of Object.entries(ids)) {
    const own =
      id === "navree"
        ? {}
        : colours(lines.find((line) => line.startsWith(`:where([data-theme=${id}]){`)) ?? "");
    if (id !== "navree" && Object.keys(own).length === 0) {
      throw new Error(`navree-ui's tokens.css has no block for the ${id} theme`);
    }
    const merged = { ...defaults, ...own };
    themes.push({
      id,
      label: names[variant] ?? id,
      description: descriptions[variant] ?? "",
      light: Object.fromEntries(COLOUR_TOKENS.map((name) => [name, merged[name].light])),
      dark: Object.fromEntries(COLOUR_TOKENS.map((name) => [name, merged[name].dark])),
    });
  }
  // Themes with their own block in the stylesheet must all be known to the Rust table.
  for (const line of lines) {
    const id = /^:where\(\[data-theme=([a-z0-9-]+)\]\)\{/.exec(line)?.[1];
    if (id && id !== "navree" && !themes.some((theme) => theme.id === id)) {
      throw new Error(`navree-ui's tokens.css has a ${id} theme that theme/mod.rs does not name`);
    }
  }
  return themes;
}

const HEADER = `Generated from navree-ui's theme table by scripts/generate-themes.mjs: do not edit by hand.`;

/**
 * @param {Theme} theme
 * @param {string[]} names
 * @param {Theme} against the colours to leave out when equal
 * @param {string} indent
 */
function declarations(theme, names, against, indent = "  ") {
  const out = [];
  for (const name of names) {
    if (against && against.light[name] === theme.light[name] && against.dark[name] === theme.dark[name]) continue;
    out.push(`${indent}--color-${name}: light-dark(${theme.light[name]}, ${theme.dark[name]});`);
  }
  return out;
}

/**
 * css/themes.css.
 * @param {Theme[]} themes
 */
export function themesCss(themes) {
  const [navree] = themes;
  if (navree?.id !== "navree") throw new Error("the first built-in theme is not navree");
  const out = [];
  out.push(`/*
 * @navree/theme/themes.css: every built-in theme, in light and dark.
 * ${HEADER}
 *
 *   @import "@navree/theme/theme.css";
 *   @import "@navree/theme/themes.css";
 *
 * Choose with attributes on <html> (or on any element, for a part of a page):
 *
 *   data-theme="navree | neutral | ocean | violet | sunset | rose | contrast"
 *   data-mode="light | dark | system"
 *
 * \`applyAppearance\` in @navree/theme sets both, and keeps the \`dark\` class that shadcn's
 * \`dark:\` classes read in step. Without the attributes the page stays on theme.css: the Navree
 * dark theme. Each colour is \`light-dark(light, dark)\`, so the mode is only \`color-scheme\`.
 */
`);
  out.push(`[data-mode="dark"] {
  color-scheme: dark;
}

[data-mode="light"] {
  color-scheme: light;
}

[data-mode="system"] {
  color-scheme: light dark;
}
`);
  out.push(`/* The Navree theme: the page's colours where a theme or a mode is set, and where Navree is asked for. */
:where(:root[data-theme], :root[data-mode], [data-theme="navree"]) {
${declarations(navree, CSS_TOKENS, null).join("\n")}
}
`);
  for (const theme of themes.slice(1)) {
    const lines = declarations(theme, CSS_TOKENS, navree);
    out.push(`/* ${theme.label}: ${theme.description} */
[data-theme="${theme.id}"] {
${lines.join("\n")}
}
`);
  }
  out.push(`/*
 * The shadcn names again where a theme or a mode is set: a variable that reads another is worked out
 * where it is declared, so a part of a page with its own theme needs its own copy.
 */
:where([data-theme], [data-mode]) {
${SEMANTIC.map(([name, value]) => `  --${name}: ${value};`).join("\n")}
  --shadow-glow-sm: 0 0 10px color-mix(in srgb, var(--color-accent) 30%, transparent);
  --shadow-glow: 0 0 18px color-mix(in srgb, var(--color-accent) 35%, transparent);
  --shadow-glow-lg: 0 0 60px -12px color-mix(in srgb, var(--color-accent) 45%, transparent);
}

:where(:root[data-theme], :root[data-mode]) ::selection {
  background: color-mix(in srgb, var(--color-accent) 28%, transparent);
}

/* Light surfaces: the pickers' icons stay dark, and a floating surface needs a lighter shadow. */
[data-mode="light"] input::-webkit-calendar-picker-indicator {
  filter: none;
}

[data-mode="light"] {
  --shadow-float: 0 30px 80px -20px rgb(10 14 12 / 0.25), 0 0 0 1px rgb(10 14 12 / 0.06);
}

@media (prefers-color-scheme: light) {
  [data-mode="system"] input::-webkit-calendar-picker-indicator {
    filter: none;
  }

  [data-mode="system"] {
    --shadow-float: 0 30px 80px -20px rgb(10 14 12 / 0.25), 0 0 0 1px rgb(10 14 12 / 0.06);
  }
}
`);
  return out.join("\n");
}

/**
 * src/themes.ts.
 * @param {Theme[]} themes
 */
export function themesTs(themes) {
  const q = (text) => JSON.stringify(text);
  const palette = (colours) =>
    COLOUR_TOKENS.map((name) => `      ${/^[a-z]+$/.test(name) ? name : q(name)}: ${q(colours[name])},`).join("\n");
  const out = [];
  out.push(`// ${HEADER}
// To change a theme, change navree-ui's table, write its files (cargo test -p navree-ui --lib
// tokens::tests::write_generated_files -- --ignored) and run \`node scripts/generate-themes.mjs\`.

/** The colours every theme sets. */
export const colourTokens = [
${COLOUR_TOKENS.map((name) => `  ${q(name)},`).join("\n")}
] as const;

export type ThemeColourToken = (typeof colourTokens)[number];

/** The colours of one theme in one mode: \`#rrggbb\`. */
export type ThemePalette = Readonly<Record<ThemeColourToken, string>>;

/** The ids of the built-in themes. */
export const themeIds = [${themes.map((theme) => q(theme.id)).join(", ")}] as const;

export type ThemeId = (typeof themeIds)[number];

export interface ThemeInfo {
  readonly id: ThemeId;
  readonly label: string;
  readonly description: string;
  readonly light: ThemePalette;
  readonly dark: ThemePalette;
}

/** The built-in themes: the same colours themes.css sets, as data. */
export const themes: Readonly<Record<ThemeId, ThemeInfo>> = {`);
  for (const theme of themes) {
    out.push(`  ${theme.id}: {
    id: ${q(theme.id)},
    label: ${q(theme.label)},
    description: ${q(theme.description)},
    light: {
${palette(theme.light)}
    },
    dark: {
${palette(theme.dark)}
    },
  },`);
  }
  out.push(`};
`);
  return out.join("\n");
}
