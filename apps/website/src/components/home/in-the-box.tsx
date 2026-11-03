import Link from "next/link";

import crates from "@/generated/crates.json";
import { cn } from "@/lib/utils";

import { ArrowLink, HomeSection, SectionHeading } from "./parts";

type Crate = { name: string; layer: number; module: string | null; description: string };

/** Where each crate is documented. */
const docs: Record<string, string> = {
  "navree-kernel": "/docs/architecture/modules/",
  "navree-macros": "/docs/basics/routing/",
  "navree-dock": "/docs/tools/dock/",
  "navree-contracts": "/docs/architecture/crate-hierarchy/",
  "navree-config": "/docs/getting-started/configuration/",
  "navree-http": "/docs/basics/routing/",
  "navree-log": "/docs/basics/logging/",
  "navree-crypto": "/docs/security/encryption-and-hashing/",
  "navree-events": "/docs/digging-deeper/events/",
  "navree-view": "/docs/digging-deeper/views/",
  "navree-cache": "/docs/digging-deeper/cache/",
  "navree-http-client": "/docs/digging-deeper/http-client/",
  "navree-schema": "/docs/database/schema-builder/",
  "navree-db": "/docs/database/getting-started/",
  "navree-orm": "/docs/orm/getting-started/",
  "navree-testing": "/docs/testing/getting-started/",
  "navree-observe": "/docs/digging-deeper/health-and-metrics/",
  "navree-api": "/docs/digging-deeper/openapi/",
  "navree-mail": "/docs/digging-deeper/mail/",
  "navree-realtime": "/docs/digging-deeper/broadcasting/",
  "navree-storage": "/docs/digging-deeper/storage/",
  "navree-session": "/docs/basics/sessions-and-csrf/",
  "navree-validation": "/docs/basics/validation/",
  "navree-audit": "/docs/security/audit-log/",
  "navree-queue": "/docs/digging-deeper/queues/",
  "navree-schedule": "/docs/digging-deeper/scheduling/",
  "navree-access": "/docs/security/authorization/",
  "navree-notify": "/docs/digging-deeper/notifications/",
  "navree-auth": "/docs/security/authentication/",
  "navree-workspace": "/docs/workspaces/introduction/",
  "navree-redis": "/docs/digging-deeper/redis/",
  "navree-mongo": "/docs/orm/mongodb/",
  "navree-time": "/docs/digging-deeper/time-zones/",
  "navree-browser": "/docs/packages/browser-testing/",
  "navree-i18n": "/docs/packages/localization/",
  "navree-settings": "/docs/packages/settings/",
  "navree-sso": "/docs/security/single-sign-on/",
  "navree-oauth": "/docs/packages/oauth-server/",
  "navree-webhooks": "/docs/packages/webhooks/",
  "navree-integrations": "/docs/packages/integrations/",
  "navree-directory": "/docs/packages/directory-sync/",
  "navree-domains": "/docs/packages/custom-domains/",
  "navree-documents": "/docs/packages/documents/",
  "navree-media": "/docs/packages/media/",
  "navree-sheets": "/docs/packages/imports-and-exports/",
  "navree-workflows": "/docs/packages/workflows/",
  "navree-search": "/docs/packages/search/",
  "navree-social": "/docs/packages/social-sign-in/",
  "navree-flags": "/docs/packages/feature-flags/",
  "navree-billing": "/docs/packages/billing/",
  "navree-ai": "/docs/ai/ai-sdk/",
  "navree-mcp": "/docs/ai/mcp-servers/",
  "navree-navigator": "/docs/ai/navigator/",
  "navree-micro": "/docs/microservices/introduction/",
  "navree-grpc": "/docs/microservices/grpc/",
  "navree-graphql": "/docs/microservices/graphql/",
  "navree-lighthouse": "/docs/tools/lighthouse/",
  "navree-fleet": "/docs/tools/fleet/",
  "navree-harbor": "/docs/tools/harbor/",
  "navree-logbook": "/docs/tools/logbook/",
  "navree-helm": "/docs/tools/helm/",
  navree: "/docs/architecture/facade-and-features/",
  "navree-cli": "/docs/digging-deeper/cli/",
};

/** What each layer is for (docs: architecture/crate-hierarchy). */
const layerNames: Record<number, string> = {
  8: "Facade and CLI",
  7: "Packages",
  6: "Ecosystem libraries",
  5: "Identity and tenancy",
  4: "Application services",
  3: "Data and I/O",
  2: "Foundation",
  1: "Contracts and config",
  0: "Kernel",
};

/**
 * Every framework crate, drawn as the decks of a hull: layer 8 (the facade
 * and the CLI) on top, the kernel at the bottom as the foundation. Generated from
 * the workspace by scripts/prebuild.ts.
 */
export function InTheBox() {
  const all = crates as Crate[];
  const layers = [...new Set(all.map((c) => c.layer))].sort((a, b) => b - a);
  return (
    <HomeSection id="in-the-box">
      <SectionHeading
        split
        id="in-the-box"
        eyebrow="In the box"
        title="Everything in the box."
        lead={
          <p>
            {all.length} crates in {layers.length} layers. A crate depends only on the layers below it, and a test checks every
            edge. Depend on the <code className="inline-code">navree</code> facade to get every module, or turn its features off
            to compile less.
          </p>
        }
        action={<ArrowLink href="/docs/architecture/crate-hierarchy/">How the layers work</ArrowLink>}
      />

      <div className="mt-12 overflow-hidden rounded-[1.25rem] border border-line bg-ink-1/40 lg:mt-14">
        {layers.map((layer, i) => {
          const isBase = layer === 0;
          const members = all.filter((c) => c.layer === layer);
          return (
            <section
              key={layer}
              aria-label={`Layer ${layer}: ${layerNames[layer] ?? ""}`}
              className={cn(
                "grid md:grid-cols-[13rem_minmax(0,1fr)] lg:grid-cols-[15rem_minmax(0,1fr)]",
                i > 0 && "border-t border-line",
                isBase && "bg-accent/[0.035] shadow-[inset_0_-1px_0_var(--color-accent)]",
              )}
            >
              <div className="flex items-baseline gap-3 px-5 pt-5 md:flex-col md:gap-1 md:border-r md:border-line md:py-5 lg:px-6">
                <span className={cn("font-mono text-xs numeric", isBase ? "text-accent" : "text-fg-3")}>L{layer}</span>
                <span className="text-[0.9375rem] font-medium text-fg-1">{layerNames[layer]}</span>
              </div>
              {/* Phones show each crate as a short chip, names only: one row per crate ran to about 3,000 px. */}
              <ul className="flex flex-wrap gap-1.5 px-5 pt-3 pb-5 sm:grid sm:grid-cols-2 sm:gap-x-2 sm:gap-y-0 sm:p-2 md:p-3 xl:grid-cols-3">
                {members.map((c) => (
                  <li key={c.name} className="min-w-0">
                    <Link
                      href={docs[c.name] ?? "/docs/architecture/crate-hierarchy/"}
                      prefetch={false}
                      className="group inline-flex rounded-lg border border-line bg-ink-0 px-2 py-1 transition-colors duration-200 hover:bg-ink-2 sm:block sm:h-full sm:rounded-xl sm:border-0 sm:bg-transparent sm:px-3 sm:py-2.5"
                    >
                      <span className="flex items-baseline justify-between gap-3">
                        <span className="font-mono text-[0.8125rem] text-fg transition-colors group-hover:text-accent sm:text-[0.84rem]">
                          {c.name}
                        </span>
                        {c.module ? <span className="truncate font-mono text-[0.6875rem] text-fg-3 max-sm:hidden">{c.module}</span> : null}
                      </span>
                      {/* Two lines each on wider screens, so the grid stays even; the whole description is still in the page. */}
                      <span title={c.description} className="mt-1 block text-[0.8125rem] leading-relaxed text-fg-3 max-sm:sr-only sm:line-clamp-2">
                        {c.description}
                      </span>
                    </Link>
                  </li>
                ))}
              </ul>
            </section>
          );
        })}
      </div>
    </HomeSection>
  );
}
