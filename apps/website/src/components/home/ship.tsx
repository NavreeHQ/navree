import { ArrowLink, HomeSection, SectionHeading } from "./parts";
import { ScriptedTerminal, type TermLine } from "./scripted-terminal";

/** A release, step by step (the Harbor docs' deploy output, shortened to fit a phone). */
const steps: [string, string, string?][] = [
  ["build", "linux x86_64, in a container", "1m 42s"],
  ["upload", "20261026-0915, 14 MB", "3s"],
  ["migrate", "2 migrations ran", "1s"],
  ["start", "green on 127.0.0.1:8082"],
  ["health", "/health/ready 200 after 0.4s"],
  ["switch", "Caddy -> green"],
  ["drain", "blue finished its requests"],
];

const session: TermLine[] = [
  { cmd: "navree deploy" },
  { out: "Deploying acme to production (1 server)", className: "text-fg-2" },
  ...steps.map(([step, detail, time]) => ({
    out: (
      <span className="flex gap-x-4">
        <span className="w-[4.5rem] shrink-0 pl-2 text-fg-1">{step}</span>
        <span className="min-w-0 flex-1">{detail}</span>
        {time ? <span className="shrink-0 text-right max-sm:hidden">{time}</span> : null}
      </span>
    ),
  })),
  { out: "Released 20261026-0915 in 1m 53s.", className: "text-accent" },
];

const facts = [
  { term: "One binary", text: "Your app, its workers and its scheduler are one executable, configured by environment variables." },
  { term: "Zero downtime", text: "The new release starts beside the old one and takes traffic only after its health check passes." },
  { term: "One-command rollback", text: "navree deploy:rollback goes back to the previous release the same way." },
  { term: "Your servers", text: "Harbor provisions any Ubuntu server over SSH: Caddy with TLS, PostgreSQL, Redis, a firewall and systemd units." },
];

/** "Ship it": one binary, and Harbor for servers and releases. */
export function Ship() {
  return (
    <HomeSection id="ship">
      <div className="grid gap-x-16 gap-y-12 lg:grid-cols-12 lg:items-center">
        <div className="lg:col-span-5">
          <SectionHeading
            id="ship"
            eyebrow="Ship it"
            title="One binary. One command to production."
            lead="Harbor, built into the CLI, sets up your servers and ships every release with a health check. Secrets stay encrypted in harbor.toml, in your repository."
          />
          <dl className="mt-9 grid gap-x-8 gap-y-6 sm:grid-cols-2">
            {facts.map((f) => (
              <div key={f.term} className="border-t border-line pt-4">
                <dt className="text-[0.9375rem] font-semibold text-fg">{f.term}</dt>
                <dd className="mt-1.5 text-sm leading-relaxed text-pretty text-fg-2">{f.text}</dd>
              </div>
            ))}
          </dl>
          <ArrowLink href="/docs/tools/harbor/" className="mt-9">
            Read about Harbor
          </ArrowLink>
        </div>

        <div className="min-w-0 lg:col-span-7">
          <div className="bezel">
            <ScriptedTerminal title="~/code/acme" lines={session} className="rounded-[calc(1.25rem-0.375rem)] border-line-2" />
          </div>
        </div>
      </div>
    </HomeSection>
  );
}
