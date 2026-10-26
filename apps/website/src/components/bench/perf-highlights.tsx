import { mb, perf } from "@/lib/bench";
import { cn } from "@/lib/utils";

import { TickerNumber } from "./ticker";

type Figure = { value: number; decimals: number; unit: string; label: string; note: string };

/** The four figures the homepage leads with, straight from Navree's results. */
export function highlightFigures(): Figure[] {
  const r = perf.results;
  const peak = mb(perf.peakMemMb);
  return [
    { value: Math.round(r.json.rps), decimals: 0, unit: "req/s", label: "Plain JSON", note: "Full request pipeline" },
    {
      value: Math.round(r.deals_list.rps),
      decimals: 0,
      unit: "req/s",
      label: "Authenticated list",
      note: "JWT, validation, a page of 20",
    },
    {
      value: peak,
      decimals: Number.isInteger(peak) ? 0 : 1,
      unit: "MB",
      label: "Peak memory",
      note: `Under load, of a ${perf.settings.appMemGb} GB limit`,
    },
    { value: perf.startupMs, decimals: 0, unit: "ms", label: "Time to ready", note: "Start to first response" },
  ];
}

/**
 * Navree's headline numbers as a grid of figures that count up once, with
 * hairlines between them. Two columns on phones, four from `lg`.
 */
export function PerfHighlights({ className }: { className?: string }) {
  const figures = highlightFigures();
  return (
    <dl
      className={cn(
        "grid grid-cols-2 gap-px overflow-hidden rounded-2xl border border-line bg-line lg:grid-cols-4",
        className,
      )}
    >
      {figures.map((f, i) => (
        <div key={f.label} className="flex flex-col bg-ink-0 p-5 sm:p-7">
          <dt className="text-sm text-fg-1">{f.label}</dt>
          <dd className="mt-6 flex flex-wrap items-baseline gap-x-2 gap-y-1 sm:mt-8">
            <TickerNumber
              value={f.value}
              decimals={f.decimals}
              delay={i * 0.08}
              className="font-display text-[2rem] leading-none font-semibold tracking-[-0.035em] text-fg sm:text-[2.75rem]"
            />
            <span className="font-mono text-xs text-fg-3 sm:text-[0.8125rem]">{f.unit}</span>
          </dd>
          <dd className="mt-3 font-mono text-[0.75rem] leading-snug text-fg-3">{f.note}</dd>
        </div>
      ))}
    </dl>
  );
}
