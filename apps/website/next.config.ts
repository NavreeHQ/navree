import type { NextConfig } from "next";

/**
 * navis.sh is a fully static site: `next build` writes plain HTML, CSS and
 * JS to `out/`, which any static host can serve (Vercel, Cloudflare Pages,
 * GitHub Pages, S3, nginx). Nothing runs on a server.
 */
const nextConfig: NextConfig = {
  output: "export",
  // `/docs/installation/` is written as `docs/installation/index.html`, which
  // every static host serves without rewrite rules.
  trailingSlash: true,
  images: { unoptimized: true },
  reactStrictMode: true,
  allowedDevOrigins: ["127.0.0.1", "localhost"],
};

export default nextConfig;
