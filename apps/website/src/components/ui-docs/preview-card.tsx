"use client";

import { useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

import { ExternalIcon } from "@/components/icons";
import { CopyButton } from "@/components/mdx/copy-button";
import { cn } from "@/lib/utils";

const TABS = ["Preview", "Code"] as const;

/**
 * The frame of `<UiPreview>`: Preview and Code tabs (WAI-ARIA tabs, arrow
 * keys move), a link that opens the preview on its own, and a copy button
 * for the Rust. Both panels are rendered on the server.
 */
export function PreviewCard({
  preview,
  code,
  codeText,
  src,
  label,
}: {
  preview: ReactNode;
  code: ReactNode;
  codeText: string;
  src: string;
  /** What the example shows, for the tab list's name. */
  label: string;
}) {
  const [active, setActive] = useState(0);
  const id = useId();
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    let next = active;
    if (event.key === "ArrowRight" || event.key === "End") next = TABS.length - 1;
    else if (event.key === "ArrowLeft" || event.key === "Home") next = 0;
    else return;
    event.preventDefault();
    setActive(next);
    tabs.current[next]?.focus();
  }

  return (
    <div className="ui-preview not-prose my-7 overflow-hidden rounded-2xl border border-line bg-ink-1 shadow-bezel">
      <div className="flex items-center justify-between gap-3 border-b border-line py-1.5 pr-1.5 pl-2">
        <div
          role="tablist"
          aria-label={label}
          onKeyDown={onKeyDown}
          className="flex items-center gap-0.5 rounded-full border border-line bg-ink-0 p-0.5"
        >
          {TABS.map((tab, i) => (
            <button
              key={tab}
              ref={(el) => {
                tabs.current[i] = el;
              }}
              type="button"
              role="tab"
              id={`${id}-tab-${i}`}
              aria-selected={i === active}
              aria-controls={`${id}-panel-${i}`}
              tabIndex={i === active ? 0 : -1}
              onClick={() => setActive(i)}
              className={cn(
                "h-7 rounded-full px-3 text-[0.8125rem] font-medium transition-colors duration-150",
                i === active ? "bg-ink-3 text-fg" : "text-fg-3 hover:text-fg-1",
              )}
            >
              {tab}
            </button>
          ))}
        </div>
        <div className="flex items-center gap-0.5">
          <a
            href={src}
            target="_blank"
            rel="noopener"
            className="inline-flex size-8 items-center justify-center rounded-md text-fg-3 transition-colors hover:bg-ink-3 hover:text-fg"
            aria-label="Open the preview on its own"
            title="Open the preview on its own"
          >
            <ExternalIcon className="size-4" />
          </a>
          <CopyButton text={codeText} label="Copy the Rust" />
        </div>
      </div>
      <div role="tabpanel" id={`${id}-panel-0`} aria-labelledby={`${id}-tab-0`} hidden={active !== 0}>
        {preview}
      </div>
      <div
        role="tabpanel"
        id={`${id}-panel-1`}
        aria-labelledby={`${id}-tab-1`}
        hidden={active !== 1}
        className="code-panel max-h-[28rem] overflow-auto"
      >
        {code}
      </div>
    </div>
  );
}
