import type { MetadataRoute } from "next";

import { site } from "@site";

export const dynamic = "force-static";

/**
 * `/manifest.webmanifest`: the name, colours and icons a browser uses when
 * the site is installed or pinned. The icons come from `scripts/icons.ts`.
 */
export default function manifest(): MetadataRoute.Manifest {
  return {
    name: site.name,
    short_name: site.name,
    description: site.tagline,
    id: "/",
    start_url: "/",
    scope: "/",
    display: "standalone",
    background_color: "#040605",
    theme_color: "#040605",
    icons: [
      { src: "/icon.svg", sizes: "any", type: "image/svg+xml", purpose: "any" },
      { src: "/icon-192.png", sizes: "192x192", type: "image/png", purpose: "any" },
      { src: "/icon-512.png", sizes: "512x512", type: "image/png", purpose: "any" },
      { src: "/icon-maskable-512.png", sizes: "512x512", type: "image/png", purpose: "maskable" },
    ],
  };
}
