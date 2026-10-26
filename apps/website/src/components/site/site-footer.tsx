import Link from "next/link";

import { site } from "@site";

import { Logo } from "./logo";

const columns: { title: string; links: { label: string; href: string }[] }[] = [
  {
    title: "Start here",
    links: [
      { label: "Introduction", href: "/docs/prologue/introduction/" },
      { label: "Installation", href: "/docs/getting-started/installation/" },
      { label: "Your first app", href: "/docs/getting-started/your-first-app/" },
      { label: "A tour of the CRM", href: "/docs/example-app/crm-tour/" },
    ],
  },
  {
    title: "Documentation",
    links: [
      { label: "Modules", href: "/docs/architecture/modules/" },
      { label: "Routing and controllers", href: "/docs/basics/routing/" },
      { label: "Authorization", href: "/docs/security/authorization/" },
      { label: "Queues", href: "/docs/digging-deeper/queues/" },
    ],
  },
  {
    title: "Project",
    links: [
      { label: "Benchmarks", href: "/benchmarks/" },
      { label: "Release notes", href: "/docs/prologue/release-notes/" },
      { label: "Roadmap", href: "/docs/prologue/roadmap/" },
      { label: "Contributing", href: "/docs/prologue/contributing/" },
      { label: "GitHub", href: site.repoUrl },
    ],
  },
];

export function SiteFooter() {
  return (
    <footer className="border-t border-line bg-ink-0">
      <div className="mx-auto grid max-w-[90rem] gap-12 px-4 py-14 sm:px-6 md:grid-cols-[1.4fr_repeat(3,1fr)] lg:px-8">
        <div className="max-w-xs">
          <Logo />
          <p className="mt-4 text-sm leading-relaxed text-fg-2">{site.tagline}</p>
        </div>
        {columns.map((column) => (
          <nav key={column.title} aria-label={column.title}>
            <h2 className="text-sm font-semibold text-fg">{column.title}</h2>
            <ul className="mt-4 space-y-2.5 text-sm">
              {column.links.map((link) => (
                <li key={link.href}>
                  {link.href.startsWith("/") ? (
                    <Link href={link.href} className="text-fg-2 transition-colors hover:text-fg">
                      {link.label}
                    </Link>
                  ) : (
                    <a href={link.href} className="text-fg-2 transition-colors hover:text-fg">
                      {link.label}
                    </a>
                  )}
                </li>
              ))}
            </ul>
          </nav>
        ))}
      </div>
      <div className="border-t border-line">
        <div className="mx-auto flex max-w-[90rem] flex-wrap items-center justify-between gap-x-6 gap-y-2 px-4 py-5 text-[0.8125rem] text-fg-3 sm:px-6 lg:px-8">
          <p>Navis is free and open source, licensed {site.license}.</p>
          <p>{site.domain}</p>
        </div>
      </div>
    </footer>
  );
}
