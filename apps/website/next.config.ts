import type { NextConfig } from "next";

/**
 * navree.com is a fully static site: `next build` writes plain HTML, CSS and
 * JS to `out/`, which any static host can serve (Vercel, Cloudflare Pages,
 * GitHub Pages, S3, nginx). Nothing runs on a server.
 *
 * A route whose page file is `page.dev.tsx` exists only under `next dev`: the
 * extension is a page extension in development and not in a build, so the
 * export holds no such route. The styleguide, which serves authors and no
 * visitor, is one. (`next/constants` names the phase; its value is used here
 * so that Node needs no module path to load this file.)
 */
const DEVELOPMENT_SERVER = "phase-development-server";
/** Next's own list. */
const PAGE_EXTENSIONS = ["tsx", "ts", "jsx", "js"];

const nextConfig = (phase: string): NextConfig => ({
  output: "export",
  pageExtensions: phase === DEVELOPMENT_SERVER ? [...PAGE_EXTENSIONS, "dev.tsx"] : PAGE_EXTENSIONS,
  // `/docs/installation/` is written as `docs/installation/index.html`, which
  // every static host serves without rewrite rules.
  trailingSlash: true,
  images: { unoptimized: true },
  reactStrictMode: true,
  allowedDevOrigins: ["127.0.0.1", "localhost"],
  turbopack: {
    resolveAlias: {
      // Next always bundles polyfills for features its own browser targets
      // already have; src/lib/no-polyfills.ts explains.
      "next/dist/build/polyfills/polyfill-module": "./src/lib/no-polyfills.ts",
      "../build/polyfills/polyfill-module": "./src/lib/no-polyfills.ts",
    },
  },
});

export default nextConfig;
