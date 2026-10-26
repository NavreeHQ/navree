import localFont from "next/font/local";

/**
 * Monaspace Neon (GitHub Next, SIL OFL), for code only. The UI and the
 * prose use the system sans. The files come from @fontsource/monaspace-neon
 * and are self-hosted by Next at build time.
 */
export const monaspace = localFont({
  src: [
    { path: "../../node_modules/@fontsource/monaspace-neon/files/monaspace-neon-latin-400-normal.woff2", weight: "400", style: "normal" },
    { path: "../../node_modules/@fontsource/monaspace-neon/files/monaspace-neon-latin-400-italic.woff2", weight: "400", style: "italic" },
    { path: "../../node_modules/@fontsource/monaspace-neon/files/monaspace-neon-latin-600-normal.woff2", weight: "600", style: "normal" },
  ],
  variable: "--font-monaspace",
  display: "swap",
  fallback: ["ui-monospace", "SFMono-Regular", "Cascadia Mono", "Menlo", "Consolas", "monospace"],
});
