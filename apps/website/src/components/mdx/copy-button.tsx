"use client";

import { useEffect, useRef, useState } from "react";

import { CheckIcon, CopyIcon } from "@/components/icons";

/**
 * Copies text to the clipboard: `text` when given, otherwise the text of
 * the `<pre>` inside the closest `[data-copy-root]` (a code block).
 */
export function CopyButton({ text, label = "Copy code", className = "" }: { text?: string; label?: string; className?: string }) {
  const [copied, setCopied] = useState(false);
  const ref = useRef<HTMLButtonElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => () => {
    if (timer.current) clearTimeout(timer.current);
  }, []);

  async function copy() {
    const value = text ?? ref.current?.closest("[data-copy-root]")?.querySelector("pre code")?.textContent ?? "";
    try {
      await navigator.clipboard.writeText(value.replace(/\n$/, ""));
      setCopied(true);
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopied(false), 1600);
    } catch {
      setCopied(false);
    }
  }

  return (
    <button
      ref={ref}
      type="button"
      onClick={copy}
      className={`inline-flex size-8 items-center justify-center rounded-md text-fg-3 transition-colors hover:bg-ink-3 hover:text-fg ${className}`}
      aria-label={copied ? "Copied" : label}
      title={copied ? "Copied" : label}
    >
      {copied ? <CheckIcon className="size-4 text-accent" /> : <CopyIcon className="size-4" />}
      <span className="sr-only" aria-live="polite">
        {copied ? "Copied to the clipboard" : ""}
      </span>
    </button>
  );
}
