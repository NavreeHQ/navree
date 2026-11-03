"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useRef, useState } from "react";

import { ChevronIcon } from "@/components/icons";
import { PlannedBadge } from "@/components/site/planned-badge";
import { cn } from "@/lib/utils";
import { site } from "@site";

/** What the sidebar needs: sections, their pages, and the extra links. */
export type SidebarNav = {
  sections: {
    slug: string;
    title: string;
    pages: { slug: string; href: string; title: string; status?: "planned" }[];
  }[];
  links: { title: string; href: string }[];
};

const normalise = (path: string) => (path.endsWith("/") ? path : `${path}/`);

/** The nearest ancestor that scrolls vertically (the sticky sidebar or the drawer). */
function scrollParent(el: HTMLElement | null): HTMLElement | null {
  for (let node = el?.parentElement ?? null; node; node = node.parentElement) {
    const { overflowY } = getComputedStyle(node);
    if (overflowY === "auto" || overflowY === "scroll") return node;
  }
  return null;
}

/**
 * The docs navigation: every section as a collapsible group, with only the
 * current section open by default and the current page marked. Used in the
 * desktop sidebar and in the mobile drawer.
 */
export function Sidebar({ nav, onNavigate }: { nav: SidebarNav; onNavigate?: () => void }) {
  const pathname = normalise(usePathname() ?? "/");
  const navRef = useRef<HTMLElement>(null);
  // Sections the reader opened or closed by hand. The rest follow the page:
  // the current section is open, the others closed.
  const [overrides, setOverrides] = useState<Record<string, boolean>>({});

  const currentSection = nav.sections.find(
    (section) => pathname === `/docs/${section.slug}/` || section.pages.some((page) => normalise(page.href) === pathname),
  )?.slug;

  // Bring the current page into view inside the sidebar (not the window).
  useEffect(() => {
    const link = navRef.current?.querySelector<HTMLElement>('[aria-current="page"]');
    const scroller = scrollParent(navRef.current);
    if (!link || !scroller) return;
    const box = scroller.getBoundingClientRect();
    const item = link.getBoundingClientRect();
    if (item.top < box.top + 48 || item.bottom > box.bottom - 48) {
      scroller.scrollTop += item.top - box.top - box.height / 3;
    }
  }, [pathname]);

  return (
    <nav ref={navRef} aria-label="Documentation" className="text-[0.875rem]">
      <ul className="flex flex-col gap-1">
        {nav.sections.map((section) => {
          const containsCurrent = section.slug === currentSection;
          const open = overrides[section.slug] ?? containsCurrent;
          const listId = `docs-nav-${section.slug}`;
          return (
            <li key={section.slug}>
              <button
                type="button"
                onClick={() => setOverrides((o) => ({ ...o, [section.slug]: !open }))}
                aria-expanded={open}
                aria-controls={listId}
                className={cn(
                  "group flex w-full cursor-pointer items-center justify-between gap-2 rounded-lg px-2 py-1.5 text-left text-[0.875rem] font-semibold transition-colors duration-200 hover:bg-ink-2",
                  containsCurrent ? "text-fg" : "text-fg-1 hover:text-fg",
                )}
              >
                <span className="flex min-w-0 items-center gap-2">
                  <span
                    aria-hidden
                    className={cn(
                      "size-1.5 shrink-0 rounded-full transition-colors",
                      containsCurrent ? "bg-accent shadow-glow-sm" : "bg-line-2 group-hover:bg-fg-3",
                    )}
                  />
                  <span className="truncate">{section.title}</span>
                </span>
                <ChevronIcon
                  className={cn(
                    "size-3.5 shrink-0 text-fg-3 transition-transform duration-200 ease-(--ease-out-quint) group-hover:text-fg-1",
                    open && "rotate-90",
                  )}
                />
              </button>
              <ul id={listId} hidden={!open} className="mt-1 mb-3 ml-[0.6875rem] flex flex-col border-l border-line">
                {section.pages.map((page) => {
                  const current = normalise(page.href) === pathname;
                  return (
                    <li key={page.slug}>
                      <Link
                        href={page.href}
                        onClick={onNavigate}
                        aria-current={current ? "page" : undefined}
                        className={cn(
                          "-ml-px flex items-center gap-2 border-l py-[0.3125rem] pr-2 pl-3.5 leading-snug transition-colors duration-150",
                          current
                            ? "rounded-r-lg border-accent bg-ink-2 font-medium text-fg shadow-[inset_1px_0_0_var(--color-accent)]"
                            : "border-transparent text-fg-2 hover:border-fg-3 hover:text-fg",
                        )}
                      >
                        <span className="min-w-0 flex-1">{page.title}</span>
                        {site.showPlannedBadges && page.status === "planned" ? <PlannedBadge compact /> : null}
                      </Link>
                    </li>
                  );
                })}
              </ul>
            </li>
          );
        })}
        {nav.links.length ? (
          <li className="mt-4 border-t border-line pt-4">
            <ul className="flex flex-col gap-0.5">
              {nav.links.map((link) => {
                const current = normalise(link.href) === pathname;
                return (
                  <li key={link.href}>
                    <Link
                      href={link.href}
                      onClick={onNavigate}
                      aria-current={current ? "page" : undefined}
                      className="flex items-center justify-between rounded-lg px-2 py-1.5 text-[0.875rem] font-medium text-fg-1 transition-colors hover:bg-ink-2 hover:text-fg"
                    >
                      {link.title}
                      <span aria-hidden className="text-fg-3">
                        &rarr;
                      </span>
                    </Link>
                  </li>
                );
              })}
            </ul>
          </li>
        ) : null}
      </ul>
    </nav>
  );
}
