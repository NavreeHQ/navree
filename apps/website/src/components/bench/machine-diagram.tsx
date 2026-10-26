import { perf } from "@/lib/bench";
import type { CpuSet } from "@/lib/bench-types";
import { cn } from "@/lib/utils";

import { FlowLine } from "./flow-line";

/** "18-23" → "18–23". */
const range = (set: CpuSet) => set.range.replace(/-/g, "–");

function CpuSquares({ set, accent }: { set: CpuSet; accent?: boolean }) {
  return (
    <span className="flex flex-wrap gap-1">
      {Array.from({ length: set.count }, (_, i) => (
        <span
          key={i}
          className={cn(
            "size-3.5 rounded-[3px] border",
            accent ? "border-accent/60 bg-accent/80 shadow-[0_0_8px_rgb(61_255_143/0.35)]" : "border-line-2 bg-ink-3",
          )}
        />
      ))}
    </span>
  );
}

function Box({
  role,
  name,
  set,
  lines,
  accent,
}: {
  role: string;
  name: string;
  set: CpuSet;
  lines: string[];
  accent?: boolean;
}) {
  return (
    <div
      className={cn(
        "flex flex-col rounded-2xl border p-5",
        accent ? "border-accent/40 bg-accent/[0.04] shadow-glow-lg" : "border-line-2 bg-ink-1",
      )}
    >
      <p className={cn("font-mono text-[0.75rem] tracking-[0.08em] uppercase", accent ? "text-accent" : "text-fg-3")}>{role}</p>
      <p className="mt-2 font-medium text-fg">{name}</p>
      <div className="mt-4">
        <CpuSquares set={set} accent={accent} />
        <p className="mt-2 font-mono text-[0.75rem] text-fg-2">
          {set.count} CPUs · {range(set)}
        </p>
      </div>
      <ul className="mt-4 space-y-1.5 border-t border-line pt-4 text-[0.8125rem] leading-snug text-fg-2">
        {lines.map((line) => (
          <li key={line}>{line}</li>
        ))}
      </ul>
    </div>
  );
}

function Connector({ label, detail, delay }: { label: string; detail: string; delay: number }) {
  return (
    <div className="relative flex h-24 items-center justify-center lg:h-auto">
      {/* Phones: a vertical line down the middle, the label beside it. */}
      <FlowLine delay={delay} className="absolute inset-y-2 left-1/2 w-2.5 -translate-x-1/2 lg:inset-x-2 lg:inset-y-auto lg:top-1/2 lg:h-2.5 lg:w-auto lg:-translate-y-1/2 lg:translate-x-0" />
      <span className="absolute top-1/2 left-[calc(50%+1.25rem)] -translate-y-1/2 font-mono text-[0.75rem] leading-snug lg:static lg:hidden">
        <span className="block text-fg-1">{label}</span>
        <span className="block text-fg-3">{detail}</span>
      </span>
      {/* From lg: the label above the line, the detail below it. */}
      <span className="absolute inset-x-0 bottom-[calc(50%+0.75rem)] hidden text-center font-mono text-[0.75rem] leading-snug text-fg-1 lg:block">
        {label}
      </span>
      <span className="absolute inset-x-0 top-[calc(50%+0.75rem)] hidden text-center font-mono text-[0.75rem] leading-snug text-fg-3 lg:block">
        {detail}
      </span>
    </div>
  );
}

/**
 * The machine: three containers on one Docker network, each pinned to its
 * own CPUs. Load flows left to right (top to bottom on phones): wrk sends
 * HTTP to the app, the app queries PostgreSQL through its pool.
 */
export function MachineDiagram() {
  const s = perf.settings;
  const title = "The benchmark machine";
  const desc = `Three containers on ${perf.machine.network}, each pinned to its own CPUs of a ${s.ncpu}-CPU ${perf.machine.cpu}. The load generator, wrk, runs on ${s.load.count} CPUs (${range(s.load)}) and holds ${s.connections} HTTP connections to the ${perf.label} app. The app runs alone on ${s.app.count} CPUs (${range(s.app)}) with a ${s.appMemGb} GB memory limit and queries PostgreSQL 17 through a pool of 64 connections. PostgreSQL runs on ${s.pg.count} CPUs (${range(s.pg)}) with its data on tmpfs.`;

  return (
    <figure role="img" aria-labelledby="machine-title" aria-describedby="machine-desc" className="m-0">
      <span id="machine-title" className="sr-only">
        {title}
      </span>
      <span id="machine-desc" className="sr-only">
        {desc}
      </span>
      <div className="grid gap-0 lg:grid-cols-[minmax(0,6fr)_7.5rem_minmax(0,8fr)_7.5rem_minmax(0,10fr)] lg:items-stretch">
        <Box
          role="Load generator"
          name="wrk 4.2.0"
          set={s.load}
          lines={[`${s.threads} threads, ${s.connections} connections`, "Monotonic clock", "10 s request timeout"]}
        />
        <Connector label="HTTP/1.1" detail={`${s.connections} connections`} delay={0} />
        <Box
          role="The app"
          name={`${perf.label}, one binary`}
          set={s.app}
          accent
          lines={[`Tokio, ${s.app.count} worker threads`, `${s.appMemGb} GB memory limit`, "Release build, log level warn"]}
        />
        <Connector label="SQL" detail="64 pooled" delay={0.9} />
        <Box
          role="Database"
          name="PostgreSQL 17"
          set={s.pg}
          lines={["Data on tmpfs", "synchronous_commit = off", "Fresh copy of the seeded database"]}
        />
      </div>
      <p className="mt-4 font-mono text-[0.75rem] text-fg-3">
        {perf.machine.cpu} · {s.ncpu} CPUs · Docker {perf.docker ?? ""} · {perf.machine.host} · {perf.machine.network}
      </p>
    </figure>
  );
}
