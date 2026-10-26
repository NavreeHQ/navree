"use client";

import { useEffect, useState } from "react";

import { cn } from "@/lib/utils";

/**
 * The bar under the header: one link per architecture on the page, the one
 * you're reading marked. It sticks below the site header and scrolls
 * sideways on its own on narrow screens (the page never does).
 */
export function PatternNav({ items }: { items: { id: string; label: string }[] }) {
  const [active, setActive] = useState<string | null>(null);

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
      aria-label="Architectures on this page"
      className="sticky top-(--header-h) z-30 border-b border-line bg-ink-0/80 backdrop-blur-xl supports-[backdrop-filter]:bg-ink-0/65"
    >
      <div className="container-site">
        <ul className="-mx-1 flex gap-1 overflow-x-auto py-2 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
          {items.map((item, i) => {
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
                  <span className={cn("font-mono text-xs tabular-nums", current ? "text-accent" : "text-fg-3")}>
                    {String(i + 1).padStart(2, "0")}
                  </span>
                  {item.label}
                </a>
              </li>
            );
          })}
        </ul>
      </div>
    </nav>
  );
}
