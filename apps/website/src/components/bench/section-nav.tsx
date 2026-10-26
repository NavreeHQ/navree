"use client";

import { useEffect, useRef, useState } from "react";

import { cn } from "@/lib/utils";

export type SectionLink = { id: string; label: string };

/**
 * The page's own navigation: a row of section links that sticks under the
 * site header and marks the section being read. On phones the row scrolls
 * sideways inside itself; the page never does.
 */
export function SectionNav({ sections, aside }: { sections: SectionLink[]; aside?: React.ReactNode }) {
  const [active, setActive] = useState<string | null>(null);
  const listRef = useRef<HTMLUListElement>(null);

  useEffect(() => {
    const targets = sections
      .map((s) => document.getElementById(s.id))
      .filter((el): el is HTMLElement => el !== null);
    if (!targets.length) return;

    // A section is "current" once its top passes a line a third of the way
    // down the viewport; the last one past that line wins. The observer's
    // area is the band above that line, so it fires exactly when a section
    // top crosses it.
    const update = () => {
      const line = window.innerHeight / 3;
      let current: string | null = null;
      for (const el of targets) {
        if (el.getBoundingClientRect().top <= line + 1) current = el.id;
      }
      setActive(current);
    };
    const observer = new IntersectionObserver(update, { rootMargin: "0px 0px -66.666% 0px", threshold: 0 });
    targets.forEach((el) => observer.observe(el));
    update();
    return () => observer.disconnect();
  }, [sections]);

  // Keep the active link visible in the row on narrow screens.
  useEffect(() => {
    if (!active || !listRef.current) return;
    const link = listRef.current.querySelector<HTMLElement>(`a[href="#${active}"]`);
    const list = listRef.current;
    if (!link) return;
    const left = link.offsetLeft - list.offsetLeft;
    if (left < list.scrollLeft || left + link.offsetWidth > list.scrollLeft + list.clientWidth) {
      list.scrollTo({ left: Math.max(0, left - 16), behavior: "smooth" });
    }
  }, [active]);

  return (
    <nav
      aria-label="On this page"
      className="sticky top-(--header-h) z-30 border-y border-line bg-ink-0/80 backdrop-blur-xl supports-[backdrop-filter]:bg-ink-0/65"
    >
      <div className="container-site flex h-12 items-center gap-6">
        <ul
          ref={listRef}
          className="-mx-1 flex min-w-0 flex-1 items-center gap-1 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
        >
          {sections.map((s) => {
            const isActive = active === s.id;
            return (
              <li key={s.id} className="shrink-0">
                <a
                  href={`#${s.id}`}
                  aria-current={isActive ? "location" : undefined}
                  className={cn(
                    "relative inline-flex h-12 items-center px-2.5 text-[0.8125rem] whitespace-nowrap transition-colors duration-200",
                    isActive ? "text-fg" : "text-fg-3 hover:text-fg-1",
                  )}
                >
                  {s.label}
                  <span
                    aria-hidden
                    className={cn(
                      "absolute inset-x-2.5 -bottom-px h-px origin-left bg-accent transition-transform duration-300 ease-(--ease-out-quint)",
                      isActive ? "scale-x-100" : "scale-x-0",
                    )}
                  />
                </a>
              </li>
            );
          })}
        </ul>
        {aside ? <div className="hidden shrink-0 md:block">{aside}</div> : null}
      </div>
    </nav>
  );
}
