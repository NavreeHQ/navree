import Link from "next/link";

import { site } from "@site";

import { Logo } from "./logo";

type FooterLink = { label: string; href: string };

const columns: { title: string; links: FooterLink[] }[] = [
  {
    title: "Docs",
    links: [
      { label: "Introduction", href: "/docs/prologue/introduction/" },
      { label: "Installation", href: "/docs/getting-started/installation/" },
      { label: "Your first app", href: "/docs/getting-started/your-first-app/" },
      { label: "Modules", href: "/docs/architecture/modules/" },
      { label: "Routing", href: "/docs/basics/routing/" },
      { label: "Workspaces", href: "/docs/workspaces/introduction/" },
      { label: "Architecture patterns", href: "/docs/patterns/overview/" },
    ],
  },
  {
    title: "Ecosystem",
    links: [
      { label: "All packages and tools", href: "/ecosystem/" },
      { label: "Lighthouse", href: "/docs/tools/lighthouse/" },
      { label: "Fleet", href: "/docs/tools/fleet/" },
      { label: "Helm", href: "/docs/tools/helm/" },
      { label: "Harbor", href: "/docs/tools/harbor/" },
      { label: "Search", href: "/docs/packages/search/" },
      { label: "Billing", href: "/docs/packages/billing/" },
    ],
  },
  {
    title: "AI",
    links: [
      { label: "Navree for AI", href: "/ai/" },
      { label: "Agent skills", href: "/docs/ai/agent-skills/" },
      { label: "Navigator", href: "/docs/ai/navigator/" },
      { label: "Docs for agents", href: "/docs/ai/docs-for-agents/" },
      { label: "AI SDK", href: "/docs/ai/ai-sdk/" },
      { label: "MCP servers", href: "/docs/ai/mcp-servers/" },
    ],
  },
  {
    title: "Resources",
    links: [
      { label: "Performance", href: "/performance/" },
      { label: "Architecture", href: "/architecture/" },
      { label: "Release notes", href: "/docs/prologue/release-notes/" },
      { label: "Roadmap", href: "/docs/prologue/roadmap/" },
      { label: "GitHub", href: site.repoUrl },
      { label: "llms.txt", href: "/llms.txt" },
    ],
  },
];

/** Internal pages go through the router; files (llms.txt) and other sites are plain links. */
function FooterAnchor({ link }: { link: FooterLink }) {
  const className = "text-fg-2 transition-colors duration-200 hover:text-fg";
  const isPage = link.href.startsWith("/") && link.href.endsWith("/");
  return isPage ? (
    <Link href={link.href} className={className}>
      {link.label}
    </Link>
  ) : (
    <a href={link.href} className={className}>
      {link.label}
    </a>
  );
}

export function SiteFooter() {
  return (
    <footer className="relative border-t border-line bg-ink-0">
      {/* The waterline: a short green rule where the footer meets the page. */}
      <div aria-hidden className="container-site pointer-events-none absolute inset-x-0 -top-px">
        <div className="h-px w-24 bg-accent shadow-glow-sm" />
      </div>
      <div className="container-site grid grid-cols-2 gap-x-6 gap-y-12 py-16 sm:gap-x-8 md:grid-cols-4 lg:grid-cols-[minmax(0,1.5fr)_repeat(4,minmax(0,1fr))] lg:py-20">
        <div className="col-span-2 max-w-sm md:col-span-4 lg:col-span-1">
          <Link href="/" className="-ml-1 inline-flex rounded-md px-1 py-1" aria-label={`${site.name} home`}>
            <Logo />
          </Link>
          <p className="mt-5 font-display text-[1.125rem] leading-snug font-semibold tracking-[-0.02em] text-balance text-fg">{site.headline}</p>
          <p className="mt-2 text-sm leading-relaxed text-fg-2">{site.tagline}</p>
        </div>
        {columns.map((column) => (
          <nav key={column.title} aria-label={column.title}>
            <h2 className="font-mono text-eyebrow text-fg-3 uppercase">{column.title}</h2>
            <ul className="mt-5 flex flex-col gap-3 text-sm">
              {column.links.map((link) => (
                <li key={link.href}>
                  <FooterAnchor link={link} />
                </li>
              ))}
            </ul>
          </nav>
        ))}
      </div>
      <div className="border-t border-line">
        <div className="container-site flex flex-wrap items-center justify-between gap-x-6 gap-y-2 py-6 text-[0.8125rem] text-fg-3">
          <p>
            {site.name} is free and open source, licensed <span className="whitespace-nowrap">{site.license}</span>.
          </p>
          <p>{site.domain}</p>
        </div>
      </div>
    </footer>
  );
}
