import Link from "next/link";

import crates from "@/generated/crates.json";

type Crate = { name: string; layer: number; module: string | null; description: string };

/** Where each crate is documented. */
const docs: Record<string, string> = {
  "navree-kernel": "/docs/architecture/modules/",
  "navree-macros": "/docs/basics/routing/",
  "navree-contracts": "/docs/architecture/crate-hierarchy/",
  "navree-config": "/docs/getting-started/configuration/",
  "navree-http": "/docs/basics/routing/",
  "navree-log": "/docs/basics/logging/",
  "navree-crypto": "/docs/security/encryption-and-hashing/",
  "navree-events": "/docs/digging-deeper/events/",
  "navree-view": "/docs/digging-deeper/views/",
  "navree-cache": "/docs/digging-deeper/cache/",
  "navree-http-client": "/docs/digging-deeper/http-client/",
  "navree-db": "/docs/database/getting-started/",
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
  navree: "/docs/architecture/facade-and-features/",
  "navree-cli": "/docs/digging-deeper/cli/",
};

/**
 * Every framework crate, drawn as the decks of a hull: layer 6 (the facade
 * and the CLI) on top, the kernel at the bottom. Generated from the
 * workspace by scripts/prebuild.ts.
 */
export function InTheBox() {
  const all = crates as Crate[];
  const layers = [...new Set(all.map((c) => c.layer))].sort((a, b) => b - a);
  return (
    <div className="decks">
      {layers.map((layer) => (
        <section key={layer} className={`deck${layer === 0 ? " is-bottom" : ""}`} aria-label={`Layer ${layer}`}>
          <p className="deck-label" aria-hidden>
            L{layer}
          </p>
          <ul className="deck-crates">
            {all
              .filter((c) => c.layer === layer)
              .map((c) => (
                <li key={c.name}>
                  <Link href={docs[c.name] ?? "/docs/architecture/crate-hierarchy/"} className="crate">
                    <span className="crate-name">{c.name}</span>
                    <span className="crate-text">{c.description}</span>
                  </Link>
                </li>
              ))}
          </ul>
        </section>
      ))}
    </div>
  );
}
