import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";

import { SearchDialog } from "@/components/search/search-dialog";
import { SiteFooter } from "@/components/site/site-footer";
import { SiteHeader } from "@/components/site/site-header";
import { site } from "@site";

import { monaspace } from "./fonts";
import "./globals.css";

export const metadata: Metadata = {
  metadataBase: new URL(site.url),
  title: {
    default: `${site.name}: the complete web framework for Rust`,
    template: `%s | ${site.name}`,
  },
  description: site.description,
  applicationName: site.name,
  openGraph: {
    type: "website",
    siteName: site.name,
    url: site.url,
    title: `${site.name}: the complete web framework for Rust`,
    description: site.tagline,
    images: [{ url: "/og.png", width: 1200, height: 630, alt: `${site.name}: ${site.tagline}` }],
  },
  twitter: { card: "summary_large_image", images: ["/og.png"] },
  icons: { apple: "/apple-touch-icon.png" },
};

export const viewport: Viewport = {
  themeColor: "#040605",
  colorScheme: "dark",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" className={monaspace.variable}>
      <body className="flex min-h-dvh flex-col">
        <a href="#main" className="skip-link">
          Skip to content
        </a>
        <SiteHeader />
        <main id="main" className="flex flex-1 flex-col outline-none" tabIndex={-1}>
          {children}
        </main>
        <SiteFooter />
        <SearchDialog />
      </body>
    </html>
  );
}
