/**
 * `@navree/theme`: navree.com's dark theme as data, for code that can't read
 * CSS variables (charts on a canvas, `<meta name="theme-color">`, emails,
 * PDFs). The CSS lives next to it: `@navree/theme/theme.css` (Tailwind 4),
 * `tokens.css` (plain custom properties) and `fonts.css`.
 */

/** The color tokens (Tailwind names without `--color-`). */
export const colors = {
  "ink-0": "#040605",
  "ink-1": "#0a0e0c",
  "ink-2": "#111714",
  "ink-3": "#16201b",
  line: "#1c2420",
  "line-2": "#2a3530",
  fg: "#e9efeb",
  "fg-1": "#cfd8d3",
  "fg-2": "#a9b5ae",
  "fg-3": "#8a968f",
  accent: "#3dff8f",
  "accent-ink": "#03130a",
  amber: "#ffbf5e",
  danger: "#ff6b6b",
  bar: "#5b6862",
} as const;

export type ColorToken = keyof typeof colors;

/** The shadcn/ui and shadcn-vue semantic colors, as the theme maps them. */
export const semanticColors = {
  background: colors["ink-0"],
  foreground: colors.fg,
  card: colors["ink-1"],
  "card-foreground": colors.fg,
  popover: colors["ink-1"],
  "popover-foreground": colors.fg,
  primary: colors.accent,
  "primary-foreground": colors["accent-ink"],
  secondary: colors["ink-2"],
  "secondary-foreground": colors.fg,
  muted: colors["ink-2"],
  "muted-foreground": colors["fg-2"],
  accent: colors.accent,
  "accent-foreground": colors["accent-ink"],
  destructive: colors.danger,
  warning: colors.amber,
  success: colors.accent,
  border: colors.line,
  input: colors["line-2"],
  ring: colors.accent,
  sidebar: colors["ink-1"],
  "sidebar-foreground": colors["fg-2"],
  "sidebar-primary": colors.fg,
  "sidebar-primary-foreground": colors["ink-0"],
  "sidebar-accent": colors["ink-2"],
  "sidebar-accent-foreground": colors.fg,
  "sidebar-border": colors.line,
  "sidebar-ring": colors.accent,
  "chart-1": "#5b6862",
  "chart-2": "#6f8f7f",
  "chart-3": "#8db8a0",
  "chart-4": "#b9e3c9",
  "chart-5": colors.accent,
} as const;

/** Font stacks; the first entry of each is the variable a font loader sets. */
export const fonts = {
  sans: ["var(--font-geist)", "ui-sans-serif", "system-ui", "-apple-system", "Segoe UI Variable Text", "Segoe UI", "Roboto", "Helvetica Neue", "Arial", "sans-serif"],
  display: ["var(--font-mona)", "var(--font-geist)", "ui-sans-serif", "system-ui", "sans-serif"],
  mono: ["var(--font-monaspace)", "ui-monospace", "SFMono-Regular", "Cascadia Mono", "Menlo", "Consolas", "monospace"],
} as const;

/** The font families `fonts.css` registers (the @fontsource names). */
export const fontFamilies = {
  display: "Mona Sans Variable",
  sans: "Geist Variable",
  mono: "Monaspace Neon",
} as const;

/** App UI corners: controls and surfaces. */
export const radius = { control: "4px", surface: "6px" } as const;

/** `<meta name="theme-color">` and the browser chrome. */
export const themeColor = colors["ink-0"];

function channel(hex: string, offset: number): number {
  const value = Number.parseInt(hex.slice(offset, offset + 2), 16) / 255;
  return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
}

/** The relative luminance of a `#rrggbb` color (WCAG 2). */
export function luminance(hex: string): number {
  return 0.2126 * channel(hex, 1) + 0.7152 * channel(hex, 3) + 0.0722 * channel(hex, 5);
}

/** The WCAG contrast ratio of two `#rrggbb` colors (1 to 21). */
export function contrastRatio(a: string, b: string): number {
  const [light, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (light + 0.05) / (dark + 0.05);
}
