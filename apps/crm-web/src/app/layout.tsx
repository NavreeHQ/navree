import type { Metadata, Viewport } from "next";
import { Instrument_Sans } from "next/font/google";
import { Providers } from "./providers";
import "./globals.css";

// One family throughout; the width axis gives condensed headings and figures.
const instrument = Instrument_Sans({
  variable: "--font-instrument",
  subsets: ["latin"],
  axes: ["wdth"],
  display: "swap",
});

export const metadata: Metadata = {
  title: { default: "Keel CRM", template: "%s | Keel CRM" },
  description: "Pipeline, accounts and follow-ups for the Keel sales team.",
};

export const viewport: Viewport = {
  themeColor: [
    { media: "(prefers-color-scheme: light)", color: "#f3f5f6" },
    { media: "(prefers-color-scheme: dark)", color: "#0c181d" },
  ],
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className={`${instrument.variable} h-full`} suppressHydrationWarning>
      <body className="min-h-full">
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}
