"use client";

import { ArrowRightIcon } from "lucide-react";
import Link from "next/link";
import { useId, useRef, useState, type KeyboardEvent } from "react";

import { InlineMarkdown } from "@/components/inline-markdown";
import { CopyButton } from "@/components/mdx/copy-button";
import { cn } from "@/lib/utils";

export type ProductPanel = {
  id: string;
  label: string;
  hint: string;
  title: string;
  text: string;
  points: string[];
  href: string;
  linkText: string;
  file: string;
  /** Shiki HTML (a `<pre>`), highlighted at build time. */
  html: string;
  extra?: { file: string; html: string };
};

function CodeFrame({ file, html, className }: { file: string; html: string; className?: string }) {
  const isFile = /\.\w+$/.test(file);
  return (
    <figure
      className={cn("code-panel overflow-hidden rounded-[0.875rem] border border-line-2 bg-ink-1 shadow-bezel", className)}
      data-copy-root
    >
      <figcaption className="code-block-bar">
        <span className={isFile ? "code-block-title" : "code-block-lang font-mono"}>{file}</span>
        <CopyButton />
      </figcaption>
      <div className="[&_pre]:scroll-thin" dangerouslySetInnerHTML={{ __html: html }} />
    </figure>
  );
}

/**
 * The product half's examples: a vertical list of capabilities on wide
 * screens (a scrolling row on phones), and the selected one's code with
 * what it does.
 */
export function ProductTabs({ panels }: { panels: ProductPanel[] }) {
  const [active, setActive] = useState(0);
  const id = useId();
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);
  const panel = panels[active];

  function select(next: number, focus = false) {
    setActive(next);
    tabs.current[next]?.scrollIntoView({ block: "nearest", inline: "nearest" });
    if (focus) tabs.current[next]?.focus();
  }

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const last = panels.length - 1;
    let next = active;
    if (event.key === "ArrowRight" || event.key === "ArrowDown") next = active === last ? 0 : active + 1;
    else if (event.key === "ArrowLeft" || event.key === "ArrowUp") next = active === 0 ? last : active - 1;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = last;
    else return;
    event.preventDefault();
    select(next, true);
  }

  return (
    <div className="grid grid-cols-1 gap-8 lg:grid-cols-[16.5rem_minmax(0,1fr)] lg:gap-10 xl:grid-cols-[18rem_minmax(0,1fr)] xl:gap-14">
      <div className="-mx-4 sm:mx-0">
        <div
          role="tablist"
          aria-label="AI in your product"
          aria-orientation="vertical"
          onKeyDown={onKeyDown}
          className="flex gap-1 overflow-x-auto border-b border-line px-4 [scrollbar-width:none] sm:px-0 lg:sticky lg:top-[calc(var(--header-h)+2rem)] lg:flex-col lg:gap-0 lg:overflow-visible lg:border-b-0 lg:border-l"
        >
          {panels.map((p, i) => {
            const on = i === active;
            return (
              <button
                key={p.id}
                ref={(el) => {
                  tabs.current[i] = el;
                }}
                type="button"
                role="tab"
                id={`${id}-tab-${p.id}`}
                aria-selected={on}
                aria-controls={`${id}-panel`}
                tabIndex={on ? 0 : -1}
                onClick={() => select(i)}
                className={cn(
                  "group relative shrink-0 cursor-pointer text-left transition-colors duration-200",
                  // Phones: a row of labels with a green underline.
                  "flex h-12 items-center px-3 text-[0.9375rem] font-medium whitespace-nowrap",
                  "after:absolute after:inset-x-2 after:-bottom-px after:h-px after:bg-accent after:shadow-glow-sm after:transition-opacity",
                  // Wide screens: a list with a green bar on the left of the active item.
                  "lg:h-auto lg:flex-col lg:items-start lg:gap-1 lg:py-3.5 lg:pr-3 lg:pl-5 lg:whitespace-normal",
                  "lg:after:top-2 lg:after:right-auto lg:after:bottom-2 lg:after:left-[-1px] lg:after:h-auto lg:after:w-px",
                  on ? "text-fg after:opacity-100" : "text-fg-3 after:opacity-0 hover:text-fg-1",
                )}
              >
                <span>{p.label}</span>
                <span
                  className={cn(
                    "hidden font-mono text-xs lg:block",
                    on ? "text-fg-2" : "text-fg-3 group-hover:text-fg-2",
                  )}
                >
                  {p.hint}
                </span>
              </button>
            );
          })}
        </div>
      </div>

      <div
        key={panel.id}
        id={`${id}-panel`}
        role="tabpanel"
        aria-labelledby={`${id}-tab-${panel.id}`}
        className="min-w-0 animate-in duration-500 ease-(--ease-out-quint) fade-in-0 slide-in-from-bottom-1"
      >
        <div className="grid grid-cols-1 gap-6 xl:grid-cols-[minmax(0,1fr)_minmax(0,20rem)] xl:gap-12">
          <div>
            <h3 className="font-display text-display-md font-semibold text-balance text-fg">{panel.title}</h3>
            <p className="mt-4 max-w-2xl leading-relaxed text-pretty text-fg-2">
              <InlineMarkdown text={panel.text} />
            </p>
          </div>
          <div className="xl:pt-1.5">
            <ul className="grid gap-2.5 text-[0.9375rem] leading-snug text-fg-1">
              {panel.points.map((point) => (
                <li key={point} className="flex gap-3">
                  <span aria-hidden className="mt-[0.55em] h-px w-3 shrink-0 bg-accent" />
                  <span className="text-pretty">
                    <InlineMarkdown text={point} />
                  </span>
                </li>
              ))}
            </ul>
            <Link
              href={panel.href}
              className="group mt-5 inline-flex items-center gap-1.5 text-[0.9375rem] font-medium text-fg-1 transition-colors hover:text-accent"
            >
              {panel.linkText}
              <ArrowRightIcon
                aria-hidden
                className="size-4 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5"
              />
            </Link>
          </div>
        </div>

        <div className="bezel mt-8 min-w-0 space-y-1.5">
          <CodeFrame file={panel.file} html={panel.html} />
          {panel.extra ? <CodeFrame file={panel.extra.file} html={panel.extra.html} /> : null}
        </div>
      </div>
    </div>
  );
}
