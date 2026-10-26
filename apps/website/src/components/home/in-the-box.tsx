import Link from "next/link";

import crates from "@/generated/crates.json";

type Crate = { name: string; layer: number; module: string | null; description: string };

/** Where each crate is documented. */
const docs: Record<string, string> = {
  "navis-kernel": "/docs/architecture/modules/",
  "navis-macros": "/docs/basics/routing/",
  "navis-contracts": "/docs/architecture/crate-hierarchy/",
  "navis-config": "/docs/getting-started/configuration/",
  "navis-http": "/docs/basics/routing/",
  "navis-log": "/docs/basics/logging/",
  "navis-crypto": "/docs/security/encryption-and-hashing/",
  "navis-events": "/docs/digging-deeper/events/",
  "navis-view": "/docs/digging-deeper/views/",
  "navis-cache": "/docs/digging-deeper/cache/",
  "navis-http-client": "/docs/digging-deeper/http-client/",
  "navis-db": "/docs/database/getting-started/",
  "navis-testing": "/docs/testing/getting-started/",
  "navis-observe": "/docs/digging-deeper/health-and-metrics/",
  "navis-api": "/docs/digging-deeper/openapi/",
  "navis-mail": "/docs/digging-deeper/mail/",
  "navis-realtime": "/docs/digging-deeper/broadcasting/",
  "navis-storage": "/docs/digging-deeper/storage/",
  "navis-session": "/docs/basics/sessions-and-csrf/",
  "navis-validation": "/docs/basics/validation/",
  "navis-audit": "/docs/security/audit-log/",
  "navis-queue": "/docs/digging-deeper/queues/",
  "navis-schedule": "/docs/digging-deeper/scheduling/",
  "navis-access": "/docs/security/authorization/",
  "navis-notify": "/docs/digging-deeper/notifications/",
  "navis-auth": "/docs/security/authentication/",
  navis: "/docs/architecture/facade-and-features/",
  "navis-cli": "/docs/digging-deeper/cli/",
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
