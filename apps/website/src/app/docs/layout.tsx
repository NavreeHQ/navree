import type { ReactNode } from "react";

import { MobileDocsNav } from "@/components/docs/mobile-nav";
import { Sidebar, type SidebarNav } from "@/components/docs/sidebar";
import { navigation } from "@/lib/docs";

/** Only what the client-side navigation needs, not the pages' sources. */
function sidebarNav(): SidebarNav {
  const nav = navigation();
  return {
    sections: nav.sections.map((section) => ({
      slug: section.slug,
      title: section.title,
      pages: section.pages.map((page) => ({ slug: page.slug, href: page.href, title: page.title })),
    })),
    links: nav.links,
  };
}

export default function DocsLayout({ children }: { children: ReactNode }) {
  const nav = sidebarNav();
  return (
    <div className="mx-auto w-full max-w-[90rem] px-4 sm:px-6 lg:grid lg:grid-cols-[15.5rem_minmax(0,1fr)] lg:gap-10 lg:px-8 xl:grid-cols-[16.5rem_minmax(0,1fr)] xl:gap-14">
      <aside className="sticky top-(--header-h) hidden h-[calc(100dvh-var(--header-h))] overflow-y-auto overscroll-contain py-9 pr-3 [scrollbar-width:thin] lg:block">
        <Sidebar nav={nav} />
      </aside>
      <MobileDocsNav nav={nav} />
      <div className="min-w-0">{children}</div>
    </div>
  );
}
