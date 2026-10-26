import { fmtMs, mb, testsWithResults } from "@/lib/bench";
import { cn } from "@/lib/utils";

import { TickerNumber } from "./ticker";

/**
 * Requests per second for each test, with p50, p99 and peak memory: a table
 * whose throughput column carries a bar (one linear scale, zero baseline,
 * one series). On phones every row becomes a block.
 */
export function ThroughputBoard() {
  const rows = testsWithResults();
  const max = Math.max(...rows.map((r) => r.result.rps));

  return (
    <figure className="m-0">
      <figcaption className="text-sm text-fg-2">
        Requests per second, median of three runs, on one linear scale. Higher is better.
      </figcaption>

      <div className="mt-6 overflow-hidden rounded-2xl border border-line bg-ink-1/60" role="table" aria-label="Results per test">
        <div
          role="row"
          className="hidden grid-cols-[minmax(0,15rem)_minmax(0,1fr)_5.5rem_5.5rem_6.5rem] items-center gap-x-6 border-b border-line px-6 py-3 font-mono text-[0.75rem] tracking-[0.06em] text-fg-3 uppercase lg:grid"
        >
          <span role="columnheader">Test</span>
          <span role="columnheader">Requests per second</span>
          <span role="columnheader" className="text-right">
            p50
          </span>
          <span role="columnheader" className="text-right">
            p99
          </span>
          <span role="columnheader" className="text-right">
            Peak memory
          </span>
        </div>

        {rows.map((row, i) => {
          const r = row.result;
          const width = (r.rps / max) * 100;
          return (
            <div
              key={row.key}
              role="row"
              className={cn(
                "grid grid-cols-3 gap-x-4 gap-y-4 px-5 py-6 transition-colors duration-200 hover:bg-ink-2/50 sm:px-6 lg:grid-cols-[minmax(0,15rem)_minmax(0,1fr)_5.5rem_5.5rem_6.5rem] lg:items-center lg:gap-x-6 lg:py-5",
                i > 0 && "border-t border-line",
              )}
            >
              <div role="rowheader" className="col-span-3 min-w-0 lg:col-span-1">
                <p className="font-medium text-fg">{row.label}</p>
                <p className="mt-1 truncate font-mono text-[0.8125rem] text-fg-3">{row.path}</p>
              </div>

              <div role="cell" className="col-span-3 min-w-0 lg:col-span-1">
                <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:gap-5">
                  <span className="shrink-0 lg:w-[8rem] lg:text-right">
                    <TickerNumber
                      value={Math.round(r.rps)}
                      delay={i * 0.06}
                      align="right"
                      className="font-display text-[1.875rem] leading-none font-semibold tracking-[-0.03em] text-fg"
                    />
                    <span className="ml-1.5 font-mono text-xs text-fg-3 lg:sr-only">req/s</span>
                  </span>
                  <span className="relative block h-2 w-full min-w-0 lg:h-2.5 lg:w-auto lg:flex-1" aria-hidden>
                    <span
                      className="absolute inset-y-0 left-0 rounded-r-[4px] bg-accent/85"
                      style={{ width: `max(${width}%, 4px)` }}
                    />
                  </span>
                </div>
              </div>

              <Stat label="p50" value={`${fmtMs(r.latencyMs.p50)} ms`} />
              <Stat label="p99" value={`${fmtMs(r.latencyMs.p99)} ms`} />
              <Stat label="Peak memory" value={`${mb(r.peakMemMb)} MB`} />
            </div>
          );
        })}
      </div>
    </figure>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div role="cell" className="min-w-0 lg:text-right">
      <span className="block font-mono text-[0.75rem] tracking-[0.06em] text-fg-3 uppercase lg:sr-only">{label}</span>
      <span className="mt-1 block font-mono text-[0.9375rem] text-fg-1 tabular-nums lg:mt-0">{value}</span>
    </div>
  );
}
