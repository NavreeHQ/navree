import { localeAttributes } from "@navree/next";
import type { ReactNode } from "react";

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html {...localeAttributes("en")} className="dark">
      <body>{children}</body>
    </html>
  );
}
