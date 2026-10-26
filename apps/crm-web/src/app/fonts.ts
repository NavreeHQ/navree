import localFont from "next/font/local";

/**
 * navree.com's three faces, self-hosted: Next copies the files into the build,
 * so the CRM makes no runtime font requests.
 *
 * - Mona Sans (GitHub, SIL OFL), the display face: page titles and big
 *   figures, set semi-expanded with tight tracking (`--font-display` in
 *   globals.css).
 * - Geist (Vercel, SIL OFL), the UI and body sans.
 * - Monaspace Neon (GitHub Next, SIL OFL), for codes, keys and table headings.
 */
export const monaSans = localFont({
  src: [
    {
      path: "../../node_modules/@fontsource-variable/mona-sans/files/mona-sans-latin-standard-normal.woff2",
      weight: "200 900",
      style: "normal",
    },
  ],
  variable: "--font-mona",
  display: "swap",
  declarations: [{ prop: "font-stretch", value: "75% 125%" }],
  fallback: ["ui-sans-serif", "system-ui", "Segoe UI", "sans-serif"],
  adjustFontFallback: false,
});

export const geist = localFont({
  src: [
    {
      path: "../../node_modules/@fontsource-variable/geist/files/geist-latin-wght-normal.woff2",
      weight: "100 900",
      style: "normal",
    },
    {
      path: "../../node_modules/@fontsource-variable/geist/files/geist-latin-wght-italic.woff2",
      weight: "100 900",
      style: "italic",
    },
  ],
  variable: "--font-geist",
  display: "swap",
  fallback: [
    "ui-sans-serif",
    "system-ui",
    "-apple-system",
    "Segoe UI",
    "Roboto",
    "Helvetica Neue",
    "Arial",
    "sans-serif",
  ],
});

export const monaspace = localFont({
  src: [
    {
      path: "../../node_modules/@fontsource/monaspace-neon/files/monaspace-neon-latin-400-normal.woff2",
      weight: "400",
      style: "normal",
    },
    {
      path: "../../node_modules/@fontsource/monaspace-neon/files/monaspace-neon-latin-600-normal.woff2",
      weight: "600",
      style: "normal",
    },
  ],
  variable: "--font-monaspace",
  display: "swap",
  fallback: ["ui-monospace", "SFMono-Regular", "Cascadia Mono", "Menlo", "Consolas", "monospace"],
});

/** Every font's CSS variable, for the `<html>` element. */
export const fontVariables = [monaSans.variable, geist.variable, monaspace.variable].join(" ");
