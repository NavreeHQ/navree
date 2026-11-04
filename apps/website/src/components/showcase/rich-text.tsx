import Link from "next/link";
import type { ReactNode } from "react";

/** Text with `code` in backticks and `[links](/path/)`, as the showcase data writes it. */
export function RichText({ text }: { text: string }): ReactNode {
  const parts = text.split(/(`[^`]+`|\[[^\]]+\]\([^)]+\))/g);
  return parts.map((part, i) => {
    if (part.startsWith("`") && part.endsWith("`") && part.length > 2) {
      return (
        <code key={i} className="inline-code">
          {part.slice(1, -1)}
        </code>
      );
    }
    const link = part.match(/^\[([^\]]+)\]\(([^)]+)\)$/);
    if (link) {
      return (
        <Link key={i} href={link[2]} prefetch={false} className="link">
          {link[1]}
        </Link>
      );
    }
    return part;
  });
}
