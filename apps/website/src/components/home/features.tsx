import Link from "next/link";

type Feature = { name: string; text: string; href: string };
type Group = { title: string; href: string; features: Feature[] };

/** Grouped like the docs sidebar; every entry links to its page. */
const groups: Group[] = [
  {
    title: "Architecture",
    href: "/docs/architecture/request-lifecycle/",
    features: [
      { name: "Modules", text: "Imports, providers, controllers and exports, as in NestJS.", href: "/docs/architecture/modules/" },
      { name: "Dependency injection", text: "A type-keyed container, checked when the app boots.", href: "/docs/architecture/providers/" },
      { name: "Lifecycle hooks", text: "Run code on init, on bootstrap and on graceful shutdown.", href: "/docs/architecture/lifecycle-hooks/" },
      { name: "One crate per module", text: "One Cargo feature each: compile only what you use.", href: "/docs/architecture/facade-and-features/" },
    ],
  },
  {
    title: "The basics",
    href: "/docs/basics/routing/",
    features: [
      { name: "Controllers and routing", text: "Attribute routes with prefixes and API versions.", href: "/docs/basics/routing/" },
      { name: "Deny by default", text: "Every route declares who may call it.", href: "/docs/basics/access-control/" },
      { name: "Validation", text: "garde rules with field-level messages.", href: "/docs/basics/validation/" },
      { name: "Errors and resources", text: "NestJS-style JSON errors and stable response shapes.", href: "/docs/basics/errors/" },
      { name: "Safe defaults", text: "Rate limits, security headers and body limits, on by default.", href: "/docs/basics/rate-limiting/" },
    ],
  },
  {
    title: "Security",
    href: "/docs/security/authentication/",
    features: [
      { name: "Authentication", text: "JWT access tokens with rotating refresh tokens.", href: "/docs/security/authentication/" },
      { name: "Account flows", text: "Registration, password reset, email verification and 2FA.", href: "/docs/security/account-flows/" },
      { name: "Authorization", text: "Roles, permissions and CASL-compatible abilities.", href: "/docs/security/authorization/" },
      { name: "Encryption and hashing", text: "AES-256-GCM with key rotation, Argon2id passwords.", href: "/docs/security/encryption-and-hashing/" },
      { name: "Audit log", text: "Data changes, sign-ins and denials, recorded by default.", href: "/docs/security/audit-log/" },
    ],
  },
  {
    title: "Database",
    href: "/docs/database/getting-started/",
    features: [
      { name: "SQLite and PostgreSQL", text: "SeaORM underneath, SQLite with no setup.", href: "/docs/database/getting-started/" },
      { name: "Unit of work", text: "One transaction per request, committed on success.", href: "/docs/database/unit-of-work/" },
      { name: "Migrations and seeders", text: "Schema changes, demo data and model factories.", href: "/docs/database/migrations/" },
      { name: "Pagination", text: "Paginated queries with a standard page shape.", href: "/docs/database/pagination/" },
    ],
  },
  {
    title: "Digging deeper",
    href: "/docs/digging-deeper/queues/",
    features: [
      { name: "Queues and scheduling", text: "Database-backed jobs with retries, and cron-style tasks.", href: "/docs/digging-deeper/queues/" },
      { name: "Events", text: "A typed event bus with queued listeners.", href: "/docs/digging-deeper/events/" },
      { name: "Mail and notifications", text: "Templated mail, database and real-time notifications.", href: "/docs/digging-deeper/notifications/" },
      { name: "Real time", text: "A Pusher-protocol WebSocket server: Laravel Echo just works.", href: "/docs/digging-deeper/broadcasting/" },
      { name: "Cache and storage", text: "Cache stores with tags and locks; disks with signed URLs.", href: "/docs/digging-deeper/cache/" },
      { name: "OpenAPI", text: "An OpenAPI 3.1 document generated from your routes.", href: "/docs/digging-deeper/openapi/" },
      { name: "Health and metrics", text: "Readiness checks and Prometheus metrics.", href: "/docs/digging-deeper/health-and-metrics/" },
      { name: "The CLI", text: "navree new, generators, and a dev server that restarts on save.", href: "/docs/digging-deeper/cli/" },
    ],
  },
  {
    title: "Testing",
    href: "/docs/testing/getting-started/",
    features: [
      { name: "In-process HTTP tests", text: "Boot the app and call it without a network.", href: "/docs/testing/http-tests/" },
      { name: "A database per test", text: "Each test gets its own temporary SQLite file.", href: "/docs/testing/database-tests/" },
      { name: "Fakes", text: "Queue, mail, events, the clock and outgoing HTTP.", href: "/docs/testing/fakes/" },
    ],
  },
];

const slug = (text: string) => text.toLowerCase().replace(/\s+/g, "-");

export function Features() {
  return (
    <div className="gap-x-12 md:columns-2 lg:columns-3">
      {groups.map((group) => (
        <section key={group.title} aria-labelledby={`feature-${slug(group.title)}`} className="mb-12 break-inside-avoid">
          <h3 id={`feature-${slug(group.title)}`} className="border-b border-line pb-3 text-base font-semibold text-fg">
            <Link href={group.href} className="transition-colors hover:text-accent">
              {group.title}
            </Link>
          </h3>
          <ul className="mt-1">
            {group.features.map((f) => (
              <li key={f.name}>
                <Link href={f.href} className="group block py-2.5">
                  <span className="text-[0.9375rem] font-medium text-fg-1 transition-colors group-hover:text-accent">{f.name}</span>
                  <span className="mt-0.5 block text-sm leading-relaxed text-fg-3">{f.text}</span>
                </Link>
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}
