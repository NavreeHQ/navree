"use client";

import { useEffect, useState } from "react";

import type { Heading } from "@/lib/mdx";

/**
 * The page's `##` and `###` headings. Scroll-spy: the current section is
 * the last heading that has scrolled past the top of the viewport (below
 * the sticky header); at the very bottom, the last heading.
 */
export function Toc({ headings }: { headings: Heading[] }) {
  const [active, setActive] = useState<string | null>(headings[0]?.id ?? null);

  useEffect(() => {
    if (!headings.length) return;
    let frame = 0;
    const update = () => {
      frame = 0;
      const offset = parseFloat(getComputedStyle(document.documentElement).scrollPaddingTop) || 96;
      // A heading is current once it is in the top fifth of the page below the header.
      const threshold = offset + Math.min(160, window.innerHeight * 0.2);
      let current = headings[0].id;
      for (const heading of headings) {
        const el = document.getElementById(heading.id);
        if (el && el.getBoundingClientRect().top <= threshold) current = heading.id;
      }
      const atBottom = window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 4;
      if (atBottom) current = headings[headings.length - 1].id;
      setActive(current);
    };
    const onScroll = () => {
      if (!frame) frame = requestAnimationFrame(update);
    };
    update();
    window.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("resize", onScroll);
    return () => {
      window.removeEventListener("scroll", onScroll);
      window.removeEventListener("resize", onScroll);
      if (frame) cancelAnimationFrame(frame);
    };
  }, [headings]);

  if (!headings.length) return null;

  return (
    <nav aria-label="On this page">
      <p className="text-[0.8125rem] font-semibold text-fg">On this page</p>
      <ul className="mt-3 space-y-px border-l border-line text-[0.8125rem] leading-snug">
        {headings.map((heading) => {
          const current = heading.id === active;
          return (
            <li key={heading.id}>
              <a
                href={`#${heading.id}`}
                aria-current={current ? "location" : undefined}
                className={`-ml-px block border-l py-1 pr-1 transition-colors ${heading.depth === 3 ? "pl-6" : "pl-3.5"} ${
                  current ? "border-accent text-fg" : "border-transparent text-fg-3 hover:text-fg-1"
                }`}
              >
                {heading.text}
              </a>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
