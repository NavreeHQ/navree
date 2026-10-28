import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";

import { SearchDialog } from "@/components/search/search-dialog";
import { SiteFooter } from "@/components/site/site-footer";
import { SiteHeader } from "@/components/site/site-header";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ogImage } from "@/lib/metadata";
import { JsonLd, siteGraph } from "@/lib/structured-data";
import { site } from "@site";

import { fontVariables } from "./fonts";
import "./globals.css";

export const metadata: Metadata = {
  metadataBase: new URL(site.url),
  title: {
    default: `${site.name}: ${site.headline}`,
    template: `%s | ${site.name}`,
  },
  description: site.description,
  applicationName: site.name,
  // Pages set their own canonical URL and social tags (`pageMetadata` in
  // src/lib/metadata.ts); these are the defaults. The icons and the web app
  // manifest come from the file conventions in this folder: favicon.ico,
  // icon.svg, apple-icon.png and manifest.ts (made by scripts/icons.ts).
  openGraph: {
    type: "website",
    siteName: site.name,
    locale: "en_US",
    url: "/",
    title: `${site.name}: ${site.headline}`,
    description: site.tagline,
    images: [ogImage],
  },
  twitter: { card: "summary_large_image", images: [{ url: ogImage.url, alt: ogImage.alt }] },
};

export const viewport: Viewport = {
  themeColor: "#040605",
  colorScheme: "dark",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" className={`dark ${fontVariables}`}>
      <body className="flex min-h-dvh flex-col">
        <a href="#main" className="skip-link">
          Skip to content
        </a>
        <TooltipProvider delayDuration={150}>
          <SiteHeader />
          <main id="main" className="flex flex-1 flex-col outline-none" tabIndex={-1}>
            {children}
          </main>
          <SiteFooter />
        </TooltipProvider>
        <SearchDialog />
        <JsonLd data={siteGraph()} />
      </body>
    </html>
  );
}
