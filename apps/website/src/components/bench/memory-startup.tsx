import { mb, perf, testsWithResults } from "@/lib/bench";

import { TickerNumber } from "./ticker";

/**
 * Memory under load against the container's limit, the peak of each test,
 * and the time to the first response. Two panels side by side from `lg`.
 */
export function MemoryAndStartup() {
  const rows = testsWithResults();
  const peak = mb(perf.peakMemMb);
  const limitMb = perf.settings.appMemGb * 1024;
  const share = (perf.peakMemMb / limitMb) * 100;
  const scaleMax = Math.ceil(Math.max(...rows.map((r) => r.result.peakMemMb)) / 10) * 10;

  return (
    <div className="grid gap-3 lg:grid-cols-12">
      {/* Memory */}
      <section aria-labelledby="memory-heading" className="surface p-6 sm:p-8 lg:col-span-8">
        <div className="flex flex-wrap items-start justify-between gap-x-8 gap-y-6">
          <div>
            <h3 id="memory-heading" className="font-mono text-[0.75rem] tracking-[0.08em] text-fg-3 uppercase">
              Peak memory under load
            </h3>
            <p className="mt-3 flex items-baseline gap-2">
              <TickerNumber
                value={peak}
                decimals={Number.isInteger(peak) ? 0 : 1}
                className="font-display text-[3rem] leading-none font-semibold tracking-[-0.04em] text-fg sm:text-[3.75rem]"
              />
              <span className="font-mono text-sm text-fg-3">MB</span>
            </p>
          </div>
          <p className="max-w-xs text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
            The highest reading in any test, sampled with <code className="inline-code">docker stats</code> while the load
            ran. At rest the process holds {mb(perf.idleMemMb)} MB.
          </p>
        </div>

        {/* The share of the container's memory limit. */}
        <div className="mt-8">
          <div className="relative h-3 rounded-full bg-ink-3" role="img" aria-label={`${peak} MB of the ${perf.settings.appMemGb} GB limit, ${share.toFixed(1)}%`}>
            <span
              className="absolute inset-y-0 left-0 rounded-full bg-accent shadow-glow-sm"
              style={{ width: `max(${share}%, 0.375rem)` }}
            />
          </div>
          <div className="mt-2.5 flex justify-between font-mono text-[0.75rem] text-fg-3">
            <span>
              <span className="text-fg-1">{share.toFixed(1)}%</span> of the limit
            </span>
            <span>{perf.settings.appMemGb} GB container limit</span>
          </div>
        </div>

        {/* Peak per test. */}
        <ul className="mt-8 grid gap-x-8 gap-y-3 border-t border-line pt-6 sm:grid-cols-2">
          {rows.map((row) => (
            <li key={row.key} className="grid grid-cols-[minmax(0,9.5rem)_minmax(0,1fr)_3.75rem] items-center gap-3 text-[0.8125rem]">
              <span className="truncate text-fg-2">{row.label}</span>
              <span className="relative h-1.5 rounded-full bg-ink-3" aria-hidden>
                <span
                  className="absolute inset-y-0 left-0 rounded-full bg-bar"
                  style={{ width: `${(row.result.peakMemMb / scaleMax) * 100}%` }}
                />
              </span>
              <span className="text-right font-mono text-fg-1 tabular-nums">{mb(row.result.peakMemMb)} MB</span>
            </li>
          ))}
          <li className="grid grid-cols-[minmax(0,9.5rem)_minmax(0,1fr)_3.75rem] items-center gap-3 text-[0.8125rem]">
            <span className="truncate text-fg-2">Idle</span>
            <span className="relative h-1.5 rounded-full bg-ink-3" aria-hidden>
              <span
                className="absolute inset-y-0 left-0 rounded-full bg-bar"
                style={{ width: `${(perf.idleMemMb / scaleMax) * 100}%` }}
              />
            </span>
            <span className="text-right font-mono text-fg-1 tabular-nums">{mb(perf.idleMemMb)} MB</span>
          </li>
        </ul>
      </section>

      {/* Startup */}
      <section aria-labelledby="startup-heading" className="surface flex flex-col p-6 sm:p-8 lg:col-span-4">
        <h3 id="startup-heading" className="font-mono text-[0.75rem] tracking-[0.08em] text-fg-3 uppercase">
          Time to ready
        </h3>
        <p className="mt-3 flex items-baseline gap-2">
          <TickerNumber
            value={perf.startupMs}
            className="font-display text-[3rem] leading-none font-semibold tracking-[-0.04em] text-fg sm:text-[3.75rem]"
          />
          <span className="font-mono text-sm text-fg-3">ms</span>
        </p>
        <p className="mt-6 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
          From the container starting to the first <code className="inline-code">200</code> from{" "}
          <code className="inline-code">GET /json</code>. In that time the app builds its module graph, wires
          every provider and starts listening.
        </p>
        <dl className="mt-auto grid grid-cols-2 gap-4 border-t border-line pt-6 font-mono text-[0.8125rem]">
          <div>
            <dt className="text-fg-3">Idle memory</dt>
            <dd className="mt-1 text-fg-1">{mb(perf.idleMemMb)} MB</dd>
          </div>
          <div>
            <dt className="text-fg-3">Processes</dt>
            <dd className="mt-1 text-fg-1">1</dd>
          </div>
        </dl>
      </section>
    </div>
  );
}
