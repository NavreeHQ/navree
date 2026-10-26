"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useState } from "react";

import { ChevronIcon } from "@/components/icons";

/** What the sidebar needs: sections, their pages, and the extra links. */
export type SidebarNav = {
  sections: { slug: string; title: string; pages: { slug: string; href: string; title: string }[] }[];
  links: { title: string; href: string }[];
};

const normalise = (path: string) => (path.endsWith("/") ? path : `${path}/`);

/**
 * The docs navigation: every section, collapsible, with the current page
 * marked. Used in the desktop sidebar and in the mobile drawer.
 */
export function Sidebar({ nav, onNavigate }: { nav: SidebarNav; onNavigate?: () => void }) {
  const pathname = normalise(usePathname() ?? "/");
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});

  return (
    <nav aria-label="Documentation" className="text-[0.9375rem]">
      <ul className="space-y-6">
        {nav.sections.map((section) => {
          const open = !collapsed[section.slug];
          const listId = `docs-nav-${section.slug}`;
          return (
            <li key={section.slug}>
              <button
                type="button"
                onClick={() => setCollapsed((c) => ({ ...c, [section.slug]: open }))}
                aria-expanded={open}
                aria-controls={listId}
                className="group flex w-full items-center justify-between gap-2 rounded-md py-1 text-left text-[0.875rem] font-semibold text-fg"
              >
                {section.title}
                <ChevronIcon
                  className={`size-3.5 shrink-0 text-fg-3 transition-transform duration-200 group-hover:text-fg-1 ${open ? "rotate-90" : ""}`}
                />
              </button>
              <ul id={listId} hidden={!open} className="mt-2 border-l border-line">
                {section.pages.map((page) => {
                  const current = normalise(page.href) === pathname;
                  return (
                    <li key={page.slug}>
                      <Link
                        href={page.href}
                        onClick={onNavigate}
                        aria-current={current ? "page" : undefined}
                        className={`-ml-px block border-l py-[0.3rem] pr-2 pl-4 leading-snug transition-colors ${
                          current
                            ? "border-accent font-medium text-fg shadow-[inset_1px_0_0_var(--color-accent)]"
                            : "border-transparent text-fg-2 hover:border-fg-3 hover:text-fg"
                        }`}
                      >
                        {page.title}
                      </Link>
                    </li>
                  );
                })}
              </ul>
            </li>
          );
        })}
        {nav.links.length ? (
          <li className="border-t border-line pt-5">
            <ul className="space-y-1">
              {nav.links.map((link) => {
                const current = normalise(link.href) === pathname;
                return (
                  <li key={link.href}>
                    <Link
                      href={link.href}
                      onClick={onNavigate}
                      aria-current={current ? "page" : undefined}
                      className="block rounded-md py-1 text-[0.875rem] font-semibold text-fg transition-colors hover:text-accent"
                    >
                      {link.title}
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
