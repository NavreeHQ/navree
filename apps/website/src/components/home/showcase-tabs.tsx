"use client";

import Link from "next/link";
import { useId, useRef, useState, type KeyboardEvent } from "react";

import { ArrowRightIcon } from "@/components/icons";
import { CopyButton } from "@/components/mdx/copy-button";

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

export function ShowcaseTabs({ panels }: { panels: ShowcasePanel[] }) {
  const [active, setActive] = useState(0);
  const id = useId();
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);
  const panel = panels[active];

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const last = panels.length - 1;
    let next = active;
    if (event.key === "ArrowRight") next = active === last ? 0 : active + 1;
    else if (event.key === "ArrowLeft") next = active === 0 ? last : active - 1;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = last;
    else return;
    event.preventDefault();
    setActive(next);
    tabs.current[next]?.focus();
  }

  return (
    <div>
      <div role="tablist" aria-label="Code examples" className="tabs-list" onKeyDown={onKeyDown}>
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
            onClick={() => setActive(i)}
            className="tabs-tab text-[0.9375rem]!"
          >
            {p.label}
          </button>
        ))}
      </div>

      <div
        id={`${id}-panel`}
        role="tabpanel"
        aria-labelledby={`${id}-tab-${panel.id}`}
        className="mt-8 grid gap-8 lg:grid-cols-[minmax(0,1fr)_19rem] lg:gap-12"
      >
        <div className="lg:order-2 lg:pt-2">
          <h3 className="text-xl leading-snug font-semibold tracking-[-0.015em] text-fg">{panel.title}</h3>
          <p className="mt-3 leading-relaxed text-fg-2">{panel.text}</p>
          <Link href={panel.href} className="link mt-5 inline-flex items-center gap-1.5 font-medium no-underline!">
            {panel.linkText}
            <ArrowRightIcon className="size-4" />
          </Link>
        </div>

        <div className="min-w-0 space-y-4 lg:order-1">
          <figure className="code-panel" data-copy-root>
            <figcaption className="code-block-bar">
              <span className="code-block-title">{panel.file}</span>
              <CopyButton />
            </figcaption>
            <pre className="shiki navree-night" tabIndex={0} dangerouslySetInnerHTML={{ __html: panel.html }} />
          </figure>
          {panel.extra ? (
            <figure className="code-panel" data-copy-root>
              <figcaption className="code-block-bar">
                {/* A file name (`src/main.rs`) looks like the first block's; a label such as "400 Bad Request" doesn't. */}
                <span className={/\.\w+$/.test(panel.extra.file) ? "code-block-title" : "code-block-lang"}>
                  {panel.extra.file}
                </span>
                <CopyButton />
              </figcaption>
              <pre className="shiki navree-night" tabIndex={0} dangerouslySetInnerHTML={{ __html: panel.extra.html }} />
            </figure>
          ) : null}
        </div>
      </div>
    </div>
  );
}
