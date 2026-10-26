"use client";

import { useEffect, useRef, useState } from "react";

import { cn } from "@/lib/utils";

/**
 * The category bar under the header: one link per section of the page,
 * the section you're reading marked. It sticks below the site header and
 * scrolls sideways on its own on narrow screens (the page never does).
 */
export function CategoryNav({ items }: { items: { id: string; label: string; count: number }[] }) {
  const [active, setActive] = useState<string | null>(null);
  const listRef = useRef<HTMLUListElement>(null);

  // On narrow screens, keep the current category's link in view in the bar.
  useEffect(() => {
    const list = listRef.current;
    const link = active ? list?.querySelector<HTMLElement>(`a[href="#${active}"]`) : null;
    if (!list || !link || list.scrollWidth <= list.clientWidth) return;
    const left = link.offsetLeft - (list.clientWidth - link.offsetWidth) / 2;
    list.scrollTo({ left: Math.max(0, left), behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
  }, [active]);

  useEffect(() => {
    const sections = items.map((i) => document.getElementById(i.id)).filter((el): el is HTMLElement => !!el);
    if (!sections.length) return;
    const observer = new IntersectionObserver(
      (entries) => {
        const visible = entries.filter((e) => e.isIntersecting).sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top);
        if (visible[0]) setActive(visible[0].target.id);
      },
      { rootMargin: "-30% 0px -60% 0px" },
    );
    sections.forEach((s) => observer.observe(s));
    return () => observer.disconnect();
  }, [items]);

  return (
    <nav
      aria-label="Ecosystem categories"
      className="sticky top-(--header-h) z-30 border-b border-line bg-ink-0/80 backdrop-blur-xl supports-[backdrop-filter]:bg-ink-0/65"
    >
      <div className="container-site">
        <ul ref={listRef} className="scroll-thin relative -mx-1 flex gap-1 overflow-x-auto py-2 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
          {items.map((item) => {
            const current = active === item.id;
            return (
              <li key={item.id} className="shrink-0">
                <a
                  href={`#${item.id}`}
                  aria-current={current ? "true" : undefined}
                  className={cn(
                    "group inline-flex h-9 items-center gap-2 rounded-full px-3.5 text-sm whitespace-nowrap transition-colors duration-200",
                    current ? "bg-ink-3 text-fg" : "text-fg-2 hover:bg-ink-2 hover:text-fg",
                  )}
                >
                  <span
                    aria-hidden
                    className={cn(
                      "size-1.5 rounded-full transition-colors duration-200",
                      current ? "bg-accent shadow-[0_0_8px_rgb(61_255_143/0.7)]" : "bg-line-2 group-hover:bg-fg-3",
                    )}
                  />
                  {item.label}
                  <span className="font-mono text-xs text-fg-3 tabular-nums">{item.count}</span>
                </a>
              </li>
            );
          })}
        </ul>
      </div>
    </nav>
  );
}
