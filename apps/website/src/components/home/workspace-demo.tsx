"use client";

import { ArrowDownIcon, CheckIcon, ChevronsUpDownIcon, LockIcon } from "lucide-react";
import { useState } from "react";

import { cn } from "@/lib/utils";

type Deal = { title: string; company: string; amount: string; stage: string };
type Space = { slug: string; name: string; initial: string; role: string; members: string[]; deals: Deal[] };

/** Three workspaces of one app. Each sees only its own deals (four each, so switching never changes the height). */
const spaces: Space[] = [
  {
    slug: "acme",
    name: "Acme Inc",
    initial: "A",
    role: "Owner",
    members: ["AL", "MK", "JS", "RT", "PN"],
    deals: [
      { title: "Fleet renewal", company: "Bluefin Labs", amount: "$48,000", stage: "Won" },
      { title: "Second warehouse", company: "Keystone Freight", amount: "$22,500", stage: "Proposal" },
      { title: "Pilot, three clinics", company: "Lumen Dental", amount: "$9,800", stage: "Qualified" },
      { title: "Support renewal", company: "Corvid Analytics", amount: "$14,200", stage: "Lead" },
    ],
  },
  {
    slug: "globex",
    name: "Globex",
    initial: "G",
    role: "Admin",
    members: ["HB", "OK", "SV"],
    deals: [
      { title: "Rail sensors, phase 2", company: "Meridian Rail", amount: "$131,000", stage: "Negotiation" },
      { title: "Cold-chain audit", company: "Northgate Foods", amount: "$16,400", stage: "Won" },
      { title: "Depot telemetry", company: "Harrow Transit", amount: "$27,300", stage: "Won" },
      { title: "Fleet tracking pilot", company: "Saltmarsh Logistics", amount: "$9,600", stage: "Proposal" },
    ],
  },
  {
    slug: "initech",
    name: "Initech",
    initial: "I",
    role: "Viewer",
    members: ["MW", "BL", "PG", "TS", "CS", "DR", "EN", "FJ"],
    deals: [
      { title: "Print fleet lease", company: "Orbit Print", amount: "$7,200", stage: "Lead" },
      { title: "Guest Wi-Fi rollout", company: "Pinecrest Hotels", amount: "$38,900", stage: "Proposal" },
      { title: "Data migration", company: "Driftwood Supply", amount: "$12,750", stage: "Qualified" },
      { title: "Kiosk refresh", company: "Brightline Cinemas", amount: "$6,300", stage: "Won" },
    ],
  },
];

const DOMAIN = "app.example.com";

const dollars = new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 });
const sum = (deals: Deal[]) => dollars.format(deals.reduce((total, d) => total + Number(d.amount.replace(/[$,]/g, "")), 0));

function Initial({ letter, active }: { letter: string; active?: boolean }) {
  return (
    <span
      aria-hidden
      className={cn(
        "inline-flex size-6 shrink-0 items-center justify-center rounded-md font-display text-xs font-semibold",
        active ? "bg-fg text-ink-0" : "border border-line-2 bg-ink-1 text-fg-1",
      )}
    >
      {letter}
    </span>
  );
}

/**
 * The workspace spotlight's live part: an app mock with a working workspace
 * switcher, and the resolution panel that shows how the request names the
 * workspace. Switching changes the URL, the role and the rows, never the
 * layout (every view reserves the same space).
 */
export function WorkspaceDemo() {
  const [index, setIndex] = useState(0);
  const space = spaces[index];

  const resolvers = [
    { kind: "Subdomain", value: `${space.slug}.${DOMAIN}`, matched: true },
    { kind: "Header", value: `X-Workspace: ${space.slug}` },
    { kind: "Path", value: `/api/w/${space.slug}/deals` },
    { kind: "Token claim", value: `"workspace": "${space.slug}"` },
  ];

  return (
    <div className="grid gap-3 lg:grid-cols-12">
      {/* The app. */}
      <div className="bezel flex min-w-0 flex-col lg:col-span-7">
        <div className="bezel-core flex flex-1 flex-col">
          <div className="flex h-11 items-center gap-3 border-b border-line px-4">
            <div className="flex gap-1.5" aria-hidden>
              <span className="size-2.5 rounded-full bg-line-2" />
              <span className="size-2.5 rounded-full bg-line-2" />
              <span className="size-2.5 rounded-full bg-line-2" />
            </div>
            <div className="mx-auto flex min-w-0 items-center gap-1.5 rounded-full border border-line bg-ink-0 px-3 py-1 font-mono text-xs text-fg-3">
              <LockIcon aria-hidden className="size-3 shrink-0" />
              <span className="truncate">
                <span className="rounded bg-accent/10 px-1 text-accent">{space.slug}</span>.{DOMAIN}/deals
              </span>
            </div>
            <span className="hidden w-[2.875rem] sm:block" aria-hidden />
          </div>

          <div className="grid flex-1 sm:grid-cols-[13.5rem_minmax(0,1fr)]">
            {/* Sidebar: the switcher. */}
            <div className="border-line p-2.5 max-sm:border-b sm:border-r">
              <div className="flex items-center gap-2.5 rounded-lg border border-line-2 bg-ink-2 px-2.5 py-2 max-sm:hidden">
                <Initial letter={space.initial} active />
                <span className="min-w-0 flex-1 truncate text-[0.8125rem] font-medium text-fg">{space.name}</span>
                <ChevronsUpDownIcon aria-hidden className="size-3.5 text-fg-3" />
              </div>
              <p className="mt-3 mb-1 px-2.5 font-mono text-[0.6875rem] tracking-[0.12em] text-fg-3 max-sm:hidden">WORKSPACES</p>
              <ul className="flex gap-1 sm:flex-col sm:gap-0.5" aria-label="Switch workspace">
                {spaces.map((s, i) => (
                  <li key={s.slug} className="min-w-0 flex-1 sm:flex-none">
                    <button
                      type="button"
                      aria-pressed={i === index}
                      onClick={() => setIndex(i)}
                      className={cn(
                        "flex w-full cursor-pointer items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-[0.8125rem] transition-colors duration-200 max-sm:justify-center",
                        i === index ? "bg-ink-3 text-fg" : "text-fg-2 hover:bg-ink-2 hover:text-fg",
                      )}
                    >
                      <span className="max-sm:hidden">
                        <Initial letter={s.initial} active={i === index} />
                      </span>
                      <span className="truncate">{s.name}</span>
                      <span className="ml-auto font-mono text-[0.6875rem] text-fg-3 max-sm:hidden">{s.role}</span>
                    </button>
                  </li>
                ))}
              </ul>
              <ul className="mt-4 space-y-0.5 border-t border-line pt-3 text-[0.8125rem] max-sm:hidden" aria-hidden>
                {["Deals", "Companies", "Members", "Settings"].map((item, i) => (
                  <li key={item} className={cn("rounded-lg px-2.5 py-1.5", i === 0 ? "bg-ink-2 text-fg" : "text-fg-3")}>
                    {item}
                  </li>
                ))}
              </ul>
            </div>

            {/* The deals of the current workspace only. */}
            <div className="flex min-w-0 flex-col p-4 sm:p-5">
              <p className="sr-only" aria-live="polite">
                {space.name}: {space.deals.length} deals, you are {space.role}.
              </p>
              <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
                <p className="font-display text-lg font-semibold text-fg">
                  Deals{" "}
                  <span className="ml-1 font-sans text-sm font-normal text-fg-3">
                    {space.deals.filter((d) => d.stage !== "Won").length} open
                  </span>
                </p>
                <p className="text-xs text-fg-3">
                  You are <span className="font-mono text-fg-1">{space.role}</span> in {space.name}
                </p>
              </div>
              {/* The workspace's own totals: they change with the switcher, like everything else. */}
              <dl className="mt-4 grid grid-cols-2 gap-px overflow-hidden rounded-xl border border-line bg-line">
                {[
                  { label: "Open pipeline", value: sum(space.deals.filter((d) => d.stage !== "Won")) },
                  { label: "Won this quarter", value: sum(space.deals.filter((d) => d.stage === "Won")) },
                ].map((stat) => (
                  <div key={stat.label} className="bg-ink-1 px-3.5 py-2.5">
                    <dt className="truncate text-xs text-fg-3">{stat.label}</dt>
                    <dd className="mt-0.5 font-mono text-[0.9375rem] text-fg numeric">{stat.value}</dd>
                  </div>
                ))}
              </dl>
              <ul key={space.slug} className="mt-3 divide-y divide-line rounded-xl border border-line">
                {space.deals.map((deal, i) => (
                  <li
                    key={deal.title}
                    className="row-in grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4 px-3.5 py-3"
                    style={{ animationDelay: `${i * 45}ms` }}
                  >
                    <span className="min-w-0">
                      <span className="block truncate text-[0.8125rem] font-medium text-fg">{deal.title}</span>
                      <span className="block truncate text-xs text-fg-3">{deal.company}</span>
                    </span>
                    <span className="text-right">
                      <span className="block font-mono text-[0.8125rem] text-fg-1 numeric">{deal.amount}</span>
                      <span className={cn("block text-xs", deal.stage === "Won" ? "text-fg-2" : "text-fg-3")}>{deal.stage}</span>
                    </span>
                  </li>
                ))}
              </ul>

              {/* The members of this workspace, and an invitation waiting to go out. */}
              <div className="mt-auto pt-5">
                <div className="flex items-center justify-between gap-3 rounded-xl border border-line px-3.5 py-2.5">
                  <div className="flex min-w-0 items-center gap-3">
                    <div className="flex -space-x-1.5" aria-hidden>
                      {space.members.slice(0, 4).map((m) => (
                        <span
                          key={m}
                          className="inline-flex size-7 items-center justify-center rounded-full border-2 border-ink-1 bg-ink-3 font-mono text-xs text-fg-1"
                        >
                          {m}
                        </span>
                      ))}
                    </div>
                    <span className="truncate text-xs text-fg-3">{space.members.length} members</span>
                  </div>
                  {/* Viewers can't invite: the role decides what the screen offers. */}
                  {space.role === "Viewer" ? (
                    <span className="shrink-0 rounded-full border border-line px-2.5 py-1 text-xs text-fg-3">Read only</span>
                  ) : (
                    <span className="shrink-0 rounded-full border border-line-2 px-2.5 py-1 text-xs text-fg-1">
                      Invite<span className="max-sm:hidden"> by email</span>
                    </span>
                  )}
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* How the request named the workspace. */}
      <div className="surface flex min-w-0 flex-col p-5 sm:p-6 lg:col-span-5">
        <h3 className="font-display text-display-sm font-semibold text-fg">Resolved from the request</h3>
        <p className="mt-2 text-[0.9375rem] leading-relaxed text-fg-2">
          From the subdomain, a header, a path prefix or a token claim, in the order you configure. The first match wins.
        </p>
        <ul className="mt-5 divide-y divide-line rounded-xl border border-line bg-ink-0/60">
          {resolvers.map((r) => (
            <li key={r.kind} className="grid grid-cols-[5.75rem_minmax(0,1fr)] items-center gap-3 px-3.5 py-2.5 sm:grid-cols-[6.5rem_minmax(0,1fr)]">
              <span className="text-[0.8125rem] text-fg-2">{r.kind}</span>
              <span className="flex min-w-0 items-center justify-between gap-2">
                <code className="truncate font-mono text-[0.78rem] text-fg-1">{r.value}</code>
                {r.matched ? (
                  <span className="inline-flex shrink-0 items-center gap-1 rounded-full border border-accent/25 bg-accent/10 px-1.5 py-px font-mono text-[0.6875rem] text-accent">
                    <CheckIcon aria-hidden className="size-3" strokeWidth={2.5} />
                    <span className="max-sm:sr-only">match</span>
                  </span>
                ) : null}
              </span>
            </li>
          ))}
        </ul>
        <div aria-hidden className="flex justify-center py-2.5 text-fg-3">
          <ArrowDownIcon className="size-4" />
        </div>
        <div className="rounded-xl border border-line-2 bg-ink-2 px-4 py-3.5 font-mono text-[0.78rem] leading-relaxed">
          <p className="text-fg">CurrentWorkspace</p>
          <dl className="mt-1.5 grid grid-cols-[6rem_minmax(0,1fr)] gap-y-0.5 text-fg-3">
            <dt>workspace</dt>
            <dd className="truncate text-fg-1">{space.name}</dd>
            <dt>role</dt>
            <dd className="text-fg-1">{space.role}</dd>
            <dt>isolation</dt>
            <dd className="text-fg-1">row</dd>
          </dl>
        </div>
        <p className="mt-4 text-sm leading-relaxed text-fg-3">
          Unknown workspace: <span className="font-mono text-fg-2">404</span>. Not a member: <span className="font-mono text-fg-2">403</span>.
          Both in the usual JSON error shape.
        </p>
      </div>
    </div>
  );
}
