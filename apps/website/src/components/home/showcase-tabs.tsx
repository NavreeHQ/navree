"use client";

import { useId, useRef, useState, type KeyboardEvent } from "react";

import { CopyButton } from "@/components/mdx/copy-button";
import { cn } from "@/lib/utils";

import { ArrowLink } from "./parts";

export type ShowcasePanel = {
  id: string;
  label: string;
  file: string;
  title: string;
  text: string;
  href: string;
  linkText: string;
  /** Highlighted at build time (Shiki), without its `<pre>`. */
  html: string;
  extra?: { file: string; html: string };
};

function CodeFrame({ file, html }: { file: string; html: string }) {
  // A file name (`src/main.rs`) reads as a path; a label such as "400 Bad Request" doesn't.
  const isFile = /\.\w+$/.test(file);
  return (
    <figure className="code-panel" data-copy-root>
      <figcaption className="code-block-bar">
        <span className={isFile ? "code-block-title" : "code-block-lang font-mono text-[0.8125rem]"}>{file}</span>
        <CopyButton />
      </figcaption>
      <pre
        className="shiki navree-night scroll-thin"
        tabIndex={0}
        // Shiki's output for a string that lives in this repository, highlighted at build time
        // (Shiki escapes the code it highlights).
        // nosemgrep: navree-raw-html
        dangerouslySetInnerHTML={{ __html: html }}
      />
    </figure>
  );
}

/** The code tabs: a line tab bar, the code in a bezel, and what it does beside it. */
export function ShowcaseTabs({ panels }: { panels: ShowcasePanel[] }) {
  const [active, setActive] = useState(0);
  const id = useId();
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);
  const panel = panels[active];

  function select(next: number) {
    setActive(next);
    tabs.current[next]?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const last = panels.length - 1;
    let next = active;
    if (event.key === "ArrowRight") next = active === last ? 0 : active + 1;
    else if (event.key === "ArrowLeft") next = active === 0 ? last : active - 1;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = last;
    else return;
    event.preventDefault();
    select(next);
    tabs.current[next]?.focus();
  }

  return (
    <div>
      <div className="relative -mx-4 sm:mx-0">
        <div
          role="tablist"
          aria-label="Code examples"
          onKeyDown={onKeyDown}
          className="scroll-thin flex gap-1 overflow-x-auto border-b border-line px-4 [scrollbar-width:none] sm:px-0"
        >
          {panels.map((p, i) => (
            <button
              key={p.id}
              ref={(el) => {
                tabs.current[i] = el;
              }}
              type="button"
              role="tab"
              id={`${id}-tab-${p.id}`}
              aria-selected={i === active}
              aria-controls={`${id}-panel`}
              tabIndex={i === active ? 0 : -1}
              onClick={() => select(i)}
              className={cn(
                "relative flex h-12 shrink-0 cursor-pointer items-center gap-2 rounded-t-md px-3 text-[0.9375rem] font-medium whitespace-nowrap transition-colors duration-200 sm:px-4",
                "after:absolute after:inset-x-2 after:-bottom-px after:h-px after:bg-accent after:opacity-0 after:shadow-glow-sm after:transition-opacity",
                i === active ? "text-fg after:opacity-100" : "text-fg-3 hover:text-fg-1",
              )}
            >
              <span className={cn("font-mono text-xs numeric", i === active ? "text-accent" : "text-fg-3")}>
                {String(i + 1).padStart(2, "0")}
              </span>
              {p.label}
            </button>
          ))}
        </div>
      </div>

      <div
        key={panel.id}
        id={`${id}-panel`}
        role="tabpanel"
        aria-labelledby={`${id}-tab-${panel.id}`}
        className="row-in mt-8 grid gap-8 lg:mt-10 lg:grid-cols-[minmax(0,1fr)_20rem] lg:gap-12 xl:grid-cols-[minmax(0,1fr)_22rem] xl:gap-16"
      >
        <div className="lg:sticky lg:top-[calc(var(--header-h)+2rem)] lg:order-2 lg:self-start lg:pt-3">
          <h3 className="font-display text-display-md font-semibold text-balance text-fg">{panel.title}</h3>
          <p className="mt-4 leading-relaxed text-pretty text-fg-2">{panel.text}</p>
          <ArrowLink href={panel.href} className="mt-6">
            {panel.linkText}
          </ArrowLink>
        </div>

        <div className="bezel min-w-0 space-y-1.5 lg:order-1">
          <CodeFrame file={panel.file} html={panel.html} />
          {panel.extra ? <CodeFrame file={panel.extra.file} html={panel.extra.html} /> : null}
        </div>
      </div>
    </div>
  );
}
