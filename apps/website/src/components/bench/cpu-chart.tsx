import { cpus, perf, testsWithResults } from "@/lib/bench";
import { cn } from "@/lib/utils";

/**
 * Where the CPU went in each test: the average load of the app, PostgreSQL
 * and the load generator, each as a share of the CPUs it was given. The app
 * is what the chart is about, so its meters carry the accent.
 */
export function CpuChart() {
  const rows = testsWithResults();
  const s = perf.settings;
  const columns = [
    { key: "app", label: perf.label, count: s.app.count, pct: (r: (typeof rows)[number]) => r.result.cpuPct, accent: true },
    { key: "pg", label: "PostgreSQL", count: s.pg.count, pct: (r: (typeof rows)[number]) => r.result.pgCpuPct, accent: false },
    { key: "load", label: "Load generator", count: s.load.count, pct: (r: (typeof rows)[number]) => r.result.wrkCpuPct, accent: false },
  ];

  return (
    <figure className="m-0">
      <figcaption className="text-sm text-fg-2">
        Average CPU while each test ran, as a share of the CPUs each container was given.
      </figcaption>
      <div className="mt-6 overflow-hidden rounded-2xl border border-line bg-ink-1/60">
        <table className="block w-full border-collapse text-left text-sm xl:table xl:table-fixed">
          <thead className="hidden border-b border-line xl:table-header-group">
            <tr className="font-mono text-[0.75rem] tracking-[0.06em] text-fg-3 uppercase">
              <th scope="col" className="w-[11rem] px-6 py-3 font-normal">
                Test
              </th>
              {columns.map((c) => (
                <th key={c.key} scope="col" className="px-6 py-3 font-normal">
                  {c.label} <span className="text-fg-3/80 normal-case">· {c.count} CPUs</span>
                </th>
              ))}
            </tr>
          </thead>
          <tbody className="block xl:table-row-group">
            {rows.map((row, i) => (
              <tr key={row.key} className={cn("block xl:table-row", i > 0 && "border-t border-line")}>
                <th scope="row" className="block px-5 pt-5 pb-1 font-medium text-fg xl:table-cell xl:px-6 xl:py-4 xl:font-normal xl:text-fg-1">
                  {row.label}
                </th>
                {columns.map((c) => {
                  const pct = c.pct(row);
                  const share = Math.min(100, pct / c.count);
                  return (
                    <td key={c.key} className="block px-5 py-1.5 last:pb-5 xl:table-cell xl:px-6 xl:py-4">
                      <span className="flex items-center gap-3">
                        <span className="w-28 shrink-0 text-[0.8125rem] text-fg-2 xl:hidden">{c.label}</span>
                        <span className="relative h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-ink-3" aria-hidden>
                          <span
                            className={cn("absolute inset-y-0 left-0 rounded-full", c.accent ? "bg-accent" : "bg-bar")}
                            style={{ width: `${share}%` }}
                          />
                        </span>
                        <span className="w-[5.5rem] shrink-0 text-right font-mono text-[0.8125rem] text-fg-1 tabular-nums">
                          {cpus(pct)}
                          <span className="text-fg-3"> of {c.count}</span>
                        </span>
                      </span>
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </figure>
  );
}

/** True when the app's CPUs were the busiest of the three in every test. */
export function appBusiestEverywhere(): boolean {
  const s = perf.settings;
  return testsWithResults().every((row) => {
    const app = row.result.cpuPct / s.app.count;
    return app > row.result.pgCpuPct / s.pg.count && app > row.result.wrkCpuPct / s.load.count;
  });
}
