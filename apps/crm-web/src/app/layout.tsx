import type { Metadata, Viewport } from "next";
import { fontVariables } from "./fonts";
import { Providers } from "./providers";
import "./globals.css";

export const metadata: Metadata = {
  title: { default: "Navree CRM", template: "%s | Navree CRM" },
  description: "Pipeline, accounts and follow-ups for the Navree sales team.",
};

// Dark only, like navree.com.
export const viewport: Viewport = {
  themeColor: "#040605",
  colorScheme: "dark",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className={`dark ${fontVariables} h-full`}>
      <body className="min-h-full">
        <a href="#content" className="skip-link">
          Skip to content
        </a>
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}
