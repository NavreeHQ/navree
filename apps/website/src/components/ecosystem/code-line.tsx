import { Fragment } from "react";

import { cn } from "@/lib/utils";

const TOKEN = /("[^"]*"|#\[[^\]]*\]|\b(?:impl|for|async|fn|await|let|pub|use)\b|::<[^>]+>|^navree\b|^cargo\b|^claude\b)/g;

/**
 * One line of Rust or shell, lightly tinted: attributes and turbofish types
 * in green, strings softer, keywords dimmed. Enough to read as code at a
 * glance, without a highlighter on the page.
 */
export function CodeLine({ code, className }: { code: string; className?: string }) {
  const parts = code.split(TOKEN).filter((p) => p !== "");
  return (
    <code className={cn("font-mono text-[0.8125rem] leading-6 whitespace-pre text-fg-1", className)}>
      {parts.map((part, i) => {
        if (part.startsWith("#[") || part.startsWith("::<")) {
          return (
            <span key={i} className="text-accent">
              {part}
            </span>
          );
        }
        if (part.startsWith('"')) {
          return (
            <span key={i} className="text-fg-2">
              {part}
            </span>
          );
        }
        if (/^(impl|for|async|fn|await|let|pub|use)$/.test(part)) {
          return (
            <span key={i} className="text-fg-3">
              {part}
            </span>
          );
        }
        if (/^(navree|cargo|claude)$/.test(part)) {
          return (
            <span key={i} className="text-accent">
              {part}
            </span>
          );
        }
        return <Fragment key={i}>{part}</Fragment>;
      })}
    </code>
  );
}
