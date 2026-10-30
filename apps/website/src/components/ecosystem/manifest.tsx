import { CheckIcon } from "lucide-react";
import Link from "next/link";

import { ecosystem, entry } from "@/content/ecosystem";
import { cn } from "@/lib/utils";
import { site } from "@site";

const rows: { slug: string; what: string }[] = [
  { slug: "lighthouse", what: "Monitoring and alerts" },
  { slug: "fleet", what: "Queue supervisor" },
  { slug: "harbor", what: "Servers and deploys" },
  { slug: "helm", what: "Admin panel" },
  { slug: "logbook", what: "Debugging timeline" },
  { slug: "navigator", what: "MCP for your agent" },
  { slug: "workspaces", what: "Multi-tenancy" },
  { slug: "search", what: "Full text and vectors" },
  { slug: "billing", what: "Stripe or Paddle" },
  { slug: "oauth", what: "OAuth 2.1 and OIDC" },
  { slug: "ai", what: "Agents and RAG" },
  { slug: "microservices", what: "NATS, Kafka, RabbitMQ" },
];

/**
 * The ship's manifest: what comes aboard with Navree, where each piece
 * lives in your app, and that it is included. A real table of links, not a
 * picture. The check marks light up once, in order, when the page loads.
 */
export function Manifest({ className }: { className?: string }) {
  const more = ecosystem.length - rows.length;
  return (
    <div className={cn("bezel relative", className)}>
      <div className="bezel-core @container">
        <div className="flex items-center justify-between gap-3 border-b border-line px-4 py-3 sm:px-5">
          <span className="font-mono text-xs tracking-[0.14em] text-fg-2 uppercase">Manifest</span>
          <span className="font-mono text-xs text-fg-3">navree 0.2 · {site.license}</span>
        </div>
        <table className="w-full border-collapse text-left">
          <caption className="sr-only">Some of what comes with Navree, where it lives in your app, and whether it is included</caption>
          <thead className="sr-only">
            <tr>
              <th scope="col">Package or tool</th>
              <th scope="col">What it is</th>
              <th scope="col">Where it lives</th>
              <th scope="col">Included</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row, i) => {
              const e = entry(row.slug);
              return (
                <tr key={row.slug} className="group border-b border-line/70 last:border-b-0">
                  <th scope="row" className="py-0 pl-4 font-normal sm:pl-5">
                    <Link
                      href={e.href}
                      className="-mx-1 inline-flex h-9 items-center rounded-md px-1 text-[0.875rem] font-medium text-fg transition-colors hover:text-accent"
                    >
                      {e.name}
                    </Link>
                  </th>
                  <td className="hidden py-0 pr-3 text-[0.8125rem] text-fg-3 @lg:table-cell">{row.what}</td>
                  <td className="py-0 pr-3 text-right font-mono text-xs text-fg-2 @lg:text-left">{e.mount ?? e.crate}</td>
                  <td className="w-10 py-0 pr-4 sm:pr-5">
                    <span
                      className="flex size-5 items-center justify-center rounded-full border border-accent/30 bg-accent/10 text-accent motion-safe:animate-in motion-safe:fade-in-0 motion-safe:zoom-in-50 motion-safe:fill-mode-both motion-safe:animation-duration-500"
                      style={{ animationDelay: `${450 + i * 110}ms` }}
                    >
                      <CheckIcon aria-hidden className="size-3" strokeWidth={2.5} />
                      <span className="sr-only">Included</span>
                    </span>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
        <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-2 border-t border-line bg-ink-0/40 px-4 py-3 sm:px-5">
          <a href="#operate" className="text-[0.8125rem] text-fg-2 transition-colors hover:text-fg">
            and {more} more, below
          </a>
          <div className="flex items-center gap-5 font-mono text-xs text-fg-3">
            <span>
              paid add-ons <span className="text-fg">0</span>
            </span>
            <span>
              accounts <span className="text-fg">0</span>
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}
