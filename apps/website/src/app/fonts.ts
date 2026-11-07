import localFont from "next/font/local";

/**
 * Three faces, all self-hosted: Next copies the files into the build, so the
 * site makes no runtime font requests.
 *
 * - Mona Sans (GitHub, SIL OFL), the display face for headings, cut at the
 *   semi-expanded width every heading uses (wdth 112) with weights 500 to
 *   650.
 * - Geist (Vercel, SIL OFL), the UI and body sans.
 * - Monaspace Neon (GitHub Next, SIL OFL), for code only.
 *
 * The files in `./fonts/` are subsets of the @fontsource packages made by
 * `scripts/subset-fonts.py` (Latin and the symbols the site uses); run it
 * again after changing a font package.
 *
 * Only the faces nearly every page shows above the fold are preloaded:
 * Mona Sans, Geist upright and Monaspace Neon regular. Italic and bold mono
 * load when a page uses them. next/font names a family after the exported
 * constant (`geist`, `monaspace`), so the on-demand faces join that family
 * through `declarations` and the browser picks the right file by weight and
 * style (scripts/postbuild.ts fails the build if a family has no face). Each
 * family gets a fallback face metric-matched to its web font
 * (`adjustFontFallback`), so swapping fonts moves the layout as little as it can.
 *
 * That fallback is built on Arial, which Android and Linux don't have. There a
 * swap of the body face rewraps paragraphs, and on the home page it moved the
 * film on a phone's first screen. So Geist is `optional`: preloaded, it is
 * usually there for the first paint; when it is late, the page keeps the
 * fallback for that view instead of swapping under the reader.
 */
export const monaSans = localFont({
  src: [{ path: "./fonts/mona-sans-display.woff2", weight: "500 650", style: "normal" }],
  variable: "--font-mona",
  display: "swap",
  fallback: ["ui-sans-serif", "system-ui", "Segoe UI", "sans-serif"],
});

export const geist = localFont({
  src: [{ path: "./fonts/geist.woff2", weight: "400 700", style: "normal" }],
  variable: "--font-geist",
  display: "optional",
  fallback: ["ui-sans-serif", "system-ui", "-apple-system", "Segoe UI", "Roboto", "Helvetica Neue", "Arial", "sans-serif"],
});

/** Geist italic: the same family, loaded only where italic text appears. */
export const geistItalic = localFont({
  src: [{ path: "./fonts/geist-italic.woff2", weight: "400 700", style: "italic" }],
  variable: "--font-geist-italic",
  display: "swap",
  declarations: [{ prop: "font-family", value: "geist" }],
  preload: false,
  adjustFontFallback: false,
});

export const monaspace = localFont({
  src: [{ path: "./fonts/monaspace-neon-400.woff2", weight: "400", style: "normal" }],
  variable: "--font-monaspace",
  display: "swap",
  fallback: ["ui-monospace", "SFMono-Regular", "Cascadia Mono", "Menlo", "Consolas", "monospace"],
});

/** Monaspace Neon italic (code comments) and semibold: the same family, loaded on use. */
export const monaspaceMore = localFont({
  src: [
    { path: "./fonts/monaspace-neon-400-italic.woff2", weight: "400", style: "italic" },
    { path: "./fonts/monaspace-neon-600.woff2", weight: "600", style: "normal" },
  ],
  variable: "--font-monaspace-more",
  display: "swap",
  declarations: [{ prop: "font-family", value: "monaspace" }],
  preload: false,
  adjustFontFallback: false,
});

/** Every font's CSS variable, for the `<html>` element. */
export const fontVariables = [monaSans.variable, geist.variable, geistItalic.variable, monaspace.variable, monaspaceMore.variable].join(" ");
