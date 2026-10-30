import { cn } from "@/lib/utils";

/*
 * Small pictures for the cards that have room for one. Each is decorative
 * (the card's words carry the meaning), so it is hidden from assistive
 * technology.
 */

/** Workspaces: how a request finds its workspace, its members and roles, and the isolation modes. */
export function WorkspacesVisual() {
  const members = [
    { name: "Dana Whitfield", role: "Owner" },
    { name: "Marcus Reyes", role: "Admin" },
    { name: "Priya Shah", role: "Member" },
    { name: "tom@acme.test", role: "Invited" },
  ];
  return (
    <div aria-hidden className="grid gap-3 sm:grid-cols-2">
      <div className="flex flex-col gap-2">
        <div className="rounded-md border border-line bg-ink-0/60 px-3 py-2 font-mono text-xs leading-5">
          <div className="text-fg-3">GET /api/v1/deals</div>
          <div className="text-fg-1">
            X-Workspace: <span className="text-accent">acme</span>
          </div>
        </div>
        <div className="rounded-md border border-line bg-ink-0/60 px-3 py-2 font-mono text-xs leading-5">
          <div className="text-fg-3">CurrentWorkspace</div>
          <div className="truncate text-fg-1">acme · you are Admin</div>
        </div>
        <div className="flex flex-wrap items-center gap-1.5 pt-1">
          <span className="mr-0.5 text-xs text-fg-3">Isolation</span>
          {["Row", "Schema", "Database"].map((mode, i) => (
            <span
              key={mode}
              className={cn(
                "rounded-sm border px-2 font-mono text-xs leading-5",
                i === 0 ? "border-line-2 bg-ink-3 text-fg" : "border-line text-fg-2",
              )}
            >
              {mode}
            </span>
          ))}
        </div>
      </div>
      <div className="overflow-hidden rounded-md border border-line bg-ink-0/60">
        <div className="flex items-center justify-between border-b border-line px-3 py-1.5 text-xs leading-5">
          <span className="text-fg-1">acme · members</span>
          <span className="text-fg-3">4</span>
        </div>
        {members.map((m) => (
          <div key={m.name} className="flex items-center justify-between gap-3 border-b border-line px-3 py-1.5 text-xs leading-5 last:border-b-0">
            <span className={cn("truncate", m.role === "Invited" ? "font-mono text-fg-3" : "text-fg-1")}>{m.name}</span>
            <span className={cn("shrink-0", m.role === "Invited" ? "text-fg-2" : "text-fg-3")}>{m.role}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

/** Starter kits: the three kits `navree new` can write. */
export function StarterKitsVisual() {
  const kits = [
    { kit: "saas", what: "Next.js and shadcn/ui front end, sign-in, workspaces, billing, admin" },
    { kit: "api", what: "The API alone: accounts, workspaces, OpenAPI, typed clients" },
    { kit: "micro", what: "A service on a broker, with health checks and tracing" },
  ];
  return (
    <div aria-hidden className="overflow-hidden rounded-md border border-line bg-ink-0/60">
      {kits.map((k) => (
        <div key={k.kit} className="grid grid-cols-[5.25rem_minmax(0,1fr)] gap-3 border-b border-line px-3 py-2 last:border-b-0">
          <span className="font-mono text-xs leading-5 text-accent">--kit {k.kit}</span>
          <span className="text-xs leading-5 text-fg-2">{k.what}</span>
        </div>
      ))}
    </div>
  );
}

/** Dock: `navree dock up`, as its docs print it. */
export function DockVisual() {
  const services = [
    { name: "postgres", image: "timescale/timescaledb:2.31.0-pg17", addr: "127.0.0.1:5432", ready: "2.0 s" },
    { name: "redis", image: "redis:7", addr: "127.0.0.1:6379", ready: "0.4 s" },
    { name: "mailpit", image: "axllent/mailpit:v1.31", addr: "127.0.0.1:1025", ready: "0.6 s" },
  ];
  return (
    <div aria-hidden className="rounded-md border border-line bg-ink-0/60 px-3 py-2.5 font-mono text-xs leading-5">
      <div className="text-fg">
        <span className="text-fg-3">$ </span>navree dock up
      </div>
      {services.map((s) => (
        <div key={s.name} className="grid grid-cols-[4.25rem_minmax(0,1fr)_auto] gap-2 pl-2">
          <span className="text-fg-1">{s.name}</span>
          <span className="truncate text-fg-3">{s.addr}</span>
          <span className="text-accent">ready {s.ready}</span>
        </div>
      ))}
    </div>
  );
}
