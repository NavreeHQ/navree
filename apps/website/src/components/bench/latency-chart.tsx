import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { fmtMs, perf, testsWithResults } from "@/lib/bench";
import { cn } from "@/lib/utils";

/** The axis: a log scale from 0.5 ms to 200 ms (room for labels past 100), so 1 ms and 60 ms both read. */
const MIN = 0.5;
const MAX = 200;
const TICKS = [0.5, 1, 2, 5, 10, 20, 50, 100];

/** Ticks labelled on phones too; the rest are labelled from `sm`. */
const MAJOR = new Set([1, 10, 100]);

const x = (ms: number) => ((Math.log10(ms) - Math.log10(MIN)) / (Math.log10(MAX) - Math.log10(MIN))) * 100;

const PERCENTILES = [
  ["p50", "p50"],
  ["p75", "p75"],
  ["p90", "p90"],
  ["p99", "p99"],
  ["p999", "p99.9"],
  ["max", "max"],
] as const;

/**
 * p50 and p99 latency per test as dumbbells on one log axis: the filled
 * green dot is the median, the ring is p99, and the line between them is
 * the tail. Hover or focus a row for every percentile; the table below the
 * chart has the same numbers.
 */
export function LatencyChart() {
  const rows = testsWithResults();
  return (
    <figure className="m-0">
      <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-3">
        <figcaption className="text-sm text-fg-2">
          Latency at {perf.settings.connections} open connections, log scale. Lower is better.
        </figcaption>
        <ul className="flex items-center gap-5 text-[0.8125rem] text-fg-2" aria-label="Legend">
          <li className="flex items-center gap-2">
            <span aria-hidden className="size-2.5 rounded-full bg-accent" />
            p50 (median)
          </li>
          <li className="flex items-center gap-2">
            <span aria-hidden className="size-2.5 rounded-full border-2 border-fg-1" />
            p99
          </li>
        </ul>
      </div>

      <div className="mt-6 rounded-2xl border border-line bg-ink-1/60 px-5 pt-3 pb-5 sm:px-6">
        <TooltipProvider delayDuration={150}>
          <div>
            {rows.map((row) => {
              const l = row.result.latencyMs;
              const a = x(l.p50);
              const b = x(l.p99);
              return (
                <Tooltip key={row.key}>
                  <TooltipTrigger asChild>
                    <div
                      tabIndex={0}
                      aria-label={`${row.label}: p50 ${fmtMs(l.p50)} ms, p99 ${fmtMs(l.p99)} ms`}
                      className="group grid rounded-lg outline-offset-2 md:grid-cols-[10rem_minmax(0,1fr)] md:items-stretch"
                    >
                      <span className="pt-3 text-sm text-fg-1 md:flex md:items-center md:pt-0">{row.label}</span>
                      <span className="relative block h-12 md:h-14">
                        <Gridlines />
                        <span
                          className="absolute top-1/2 h-0.5 -translate-y-1/2 bg-fg-3/70 transition-colors duration-200 group-hover:bg-fg-2"
                          style={{ left: `${a}%`, width: `${b - a}%` }}
                        />
                        <span
                          className="absolute top-1/2 size-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-accent ring-2 ring-ink-1"
                          style={{ left: `${a}%` }}
                        />
                        <span
                          className="absolute top-1/2 size-3 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-fg-1 bg-ink-1 ring-2 ring-ink-1"
                          style={{ left: `${b}%` }}
                        />
                        <span
                          className="absolute top-1/2 -translate-y-1/2 font-mono text-xs whitespace-nowrap text-fg-2 tabular-nums"
                          style={{ right: `calc(${100 - a}% + 0.75rem)` }}
                        >
                          {fmtMs(l.p50)}
                        </span>
                        <span
                          className="absolute top-1/2 -translate-y-1/2 font-mono text-xs whitespace-nowrap text-fg-1 tabular-nums"
                          style={{ left: `calc(${b}% + 0.75rem)` }}
                        >
                          {fmtMs(l.p99)} ms
                        </span>
                      </span>
                    </div>
                  </TooltipTrigger>
                  <TooltipContent side="top" className="px-3 py-2.5">
                    <p className="text-xs font-medium text-fg">{row.label}</p>
                    <dl className="mt-1.5 grid grid-cols-[auto_auto] gap-x-4 gap-y-0.5 font-mono text-xs">
                      {PERCENTILES.map(([key, label]) => (
                        <div key={key} className="contents">
                          <dt className="text-fg-3">{label}</dt>
                          <dd className="text-right text-fg-1 tabular-nums">{fmtMs(l[key])} ms</dd>
                        </div>
                      ))}
                    </dl>
                  </TooltipContent>
                </Tooltip>
              );
            })}
          </div>
        </TooltipProvider>

        {/* The axis. */}
        <div aria-hidden className="grid md:grid-cols-[10rem_minmax(0,1fr)]">
          <span className="hidden md:block" />
          <span className="relative block h-6 border-t border-line-2">
            {TICKS.map((t, i) => (
              <span
                key={t}
                className={cn(
                  "absolute top-2 font-mono text-[0.75rem] whitespace-nowrap text-fg-3 tabular-nums",
                  !MAJOR.has(t) && "max-sm:hidden",
                  i === 0 ? "" : "-translate-x-1/2",
                )}
                style={{ left: `${x(t)}%` }}
              >
                {t}
                {t === 100 ? " ms" : null}
              </span>
            ))}
          </span>
        </div>
      </div>

      <details className="group/details mt-4 text-sm">
        <summary className="w-fit cursor-pointer rounded-md text-fg-2 transition-colors hover:text-fg">
          Every percentile, as a table
        </summary>
        <div className="mt-4 overflow-x-auto rounded-xl border border-line">
          <table className="w-full min-w-[34rem] text-left text-sm">
            <thead className="border-b border-line font-mono text-[0.75rem] tracking-[0.06em] text-fg-3 uppercase">
              <tr>
                <th scope="col" className="px-4 py-2.5 font-normal">
                  Test
                </th>
                {PERCENTILES.map(([key, label]) => (
                  <th key={key} scope="col" className="px-4 py-2.5 text-right font-normal">
                    {label}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody className="divide-y divide-line">
              {rows.map((row) => (
                <tr key={row.key}>
                  <th scope="row" className="px-4 py-2.5 font-normal text-fg-1">
                    {row.label}
                  </th>
                  {PERCENTILES.map(([key]) => (
                    <td key={key} className="px-4 py-2.5 text-right font-mono text-fg-1 tabular-nums">
                      {fmtMs(row.result.latencyMs[key])}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
          <p className="border-t border-line px-4 py-2.5 text-xs text-fg-3">Milliseconds, from the median run of each test.</p>
        </div>
      </details>
    </figure>
  );
}

function Gridlines() {
  return (
    <span aria-hidden className="absolute inset-0">
      {TICKS.map((t) => (
        <span key={t} className="absolute inset-y-0 w-px bg-line" style={{ left: `${x(t)}%` }} />
      ))}
    </span>
  );
}
