import Link from "next/link";

import { entry } from "@/content/ecosystem";
import { repoBlob, site } from "@site";

import { Logo } from "./logo";

type FooterLink = { label: string; href: string };

/** The first-party tools, in the order the launch names them; names and pages come from the ecosystem catalogue. */
const tools = ["lighthouse", "logbook", "fleet", "helm", "harbor", "dock", "navigator"].map((slug) => {
  const e = entry(slug);
  return { label: e.name, href: e.href };
});

/**
 * One column per journey: what Navree is (the header's pages), how to start
 * (and bring what you have), the docs people open most, the tools, and the
 * project itself (with About, which the header also shows).
 */
const columns: { title: string; links: FooterLink[] }[] = [
  {
    title: "Product",
    links: site.nav.filter((item) => item.href !== "/docs/" && item.href !== "/about/").map((item) => ({ label: item.label, href: item.href })),
  },
  {
    title: "Start",
    links: [
      { label: "Installation", href: "/docs/getting-started/installation/" },
      { label: "Your first app", href: "/docs/getting-started/your-first-app/" },
      { label: "Starter kits", href: "/docs/getting-started/starter-kits/" },
      { label: "Choosing a UI", href: "/docs/ui/choosing-a-ui/" },
      { label: "Set up your agent", href: "/docs/ai/introduction/#set-up-your-agent" },
      { label: "Bring your product", href: "/docs/getting-started/bring-your-product/" },
      { label: "Bring your design", href: "/docs/getting-started/bring-your-design/" },
    ],
  },
  {
    title: "Docs",
    links: [
      { label: "Introduction", href: "/docs/prologue/introduction/" },
      { label: "Modules", href: "/docs/architecture/modules/" },
      { label: "Routing", href: "/docs/basics/routing/" },
      { label: "ORM", href: "/docs/orm/getting-started/" },
      { label: "Workspaces", href: "/docs/workspaces/introduction/" },
      { label: "UI components", href: "/docs/components/overview/" },
      { label: "AI SDK", href: "/docs/ai/ai-sdk/" },
    ],
  },
  { title: "Tools", links: tools },
  {
    title: "Project",
    links: [
      { label: "About", href: "/about/" },
      { label: "Release notes", href: "/docs/prologue/release-notes/" },
      { label: "Roadmap", href: "/docs/prologue/roadmap/" },
      { label: "Contributing", href: "/docs/prologue/contributing/" },
      { label: "Security policy", href: repoBlob("SECURITY.md") },
      { label: "GitHub", href: site.repoUrl },
      { label: "llms.txt", href: "/llms.txt" },
    ],
  },
];

/**
 * Internal pages go through the router; files (llms.txt) and other sites are
 * plain links. The footer's pages aren't prefetched: reaching the bottom of a
 * page would otherwise fetch every one of them.
 */
function FooterAnchor({ link }: { link: FooterLink }) {
  const className = "text-fg-2 transition-colors duration-200 hover:text-fg";
  const isPage = link.href.startsWith("/") && /\/(#[\w-]+)?$/.test(link.href);
  return isPage ? (
    <Link href={link.href} prefetch={false} className={className}>
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
      <div className="container-site grid grid-cols-2 gap-x-6 gap-y-12 py-16 sm:gap-x-8 md:grid-cols-3 lg:grid-cols-5 lg:py-20 xl:grid-cols-[minmax(0,1.4fr)_repeat(5,minmax(0,1fr))]">
        <div className="col-span-2 max-w-sm md:col-span-3 lg:col-span-5 xl:col-span-1">
          <Link href="/" prefetch={false} className="-ml-1 inline-flex rounded-md px-1 py-1" aria-label={`${site.name} home`}>
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
