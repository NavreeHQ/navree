import { fmtInt, fmtPct, perf, testsWithResults } from "@/lib/bench";

/**
 * Run-to-run spread: for each test, the three measured runs and the repeat
 * run as a percentage of the reported median, on one axis around zero. The
 * repeat's change sits in its own column, so no label meets a dot.
 */
export function RepeatChart() {
  const repeat = perf.repeat;
  const rows = testsWithResults().map((row) => {
    const runs = row.result.runs.map((run) => ((run.rps - row.result.rps) / row.result.rps) * 100);
    const again = repeat?.tests.find((t) => t.key === row.key);
    const second = again ? ((again.second - row.result.rps) / row.result.rps) * 100 : null;
    return { ...row, runs, second, secondRps: again?.second ?? null };
  });
  const extent = Math.max(2, ...rows.flatMap((r) => [...r.runs.map(Math.abs), Math.abs(r.second ?? 0)]));
  const domain = Math.ceil(extent + 0.5);
  const x = (pct: number) => ((pct + domain) / (2 * domain)) * 100;
  const step = domain <= 4 ? 1 : 2;
  const ticks = Array.from({ length: Math.floor((domain * 2) / step) + 1 }, (_, i) => -domain + i * step);
  const grid = "md:grid-cols-[10rem_minmax(0,1fr)_4rem]";

  return (
    <figure className="m-0">
      <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-3">
        <figcaption className="text-sm text-fg-2">Each run against the reported median, in percent.</figcaption>
        <ul className="flex flex-wrap items-center gap-x-5 gap-y-2 text-[0.8125rem] text-fg-2" aria-label="Legend">
          <li className="flex items-center gap-2">
            <span aria-hidden className="size-2.5 rounded-full bg-fg-2" />
            The {perf.settings.runs} measured runs
          </li>
          {repeat ? (
            <li className="flex items-center gap-2">
              <span aria-hidden className="size-2.5 rounded-full border-2 border-accent" />
              Repeat, {repeat.minutesLater} minutes later
            </li>
          ) : null}
        </ul>
      </div>

      <div className="mt-6 rounded-2xl border border-line bg-ink-1/60 px-5 pt-3 pb-5 sm:px-6">
        <div
          aria-hidden
          className={`hidden border-b border-line pt-1 pb-2.5 font-mono text-[0.75rem] tracking-[0.06em] text-fg-3 uppercase md:grid ${grid}`}
        >
          <span>Test</span>
          <span />
          <span className="text-right">Repeat</span>
        </div>
        <ul>
          {rows.map((row) => (
            <li key={row.key} className={`grid grid-cols-[minmax(0,1fr)_auto] ${grid} md:items-stretch`}>
              <span className="pt-3 text-sm text-fg-1 md:flex md:items-center md:pt-0">{row.label}</span>
              <span
                aria-hidden
                className="pt-3 text-right font-mono text-xs text-fg-1 tabular-nums md:order-last md:flex md:items-center md:justify-end md:pt-0"
              >
                {row.second !== null ? fmtPct(row.second) : ""}
              </span>
              <span className="relative col-span-2 block h-10 md:col-span-1 md:h-12" aria-hidden>
                {ticks.map((t) => (
                  <span
                    key={t}
                    className={t === 0 ? "absolute inset-y-0 w-px bg-line-2" : "absolute inset-y-0 w-px bg-line"}
                    style={{ left: `${x(t)}%` }}
                  />
                ))}
                {row.runs.map((pct, i) => (
                  <span
                    key={i}
                    className="absolute top-1/2 size-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-fg-2 ring-2 ring-ink-1"
                    style={{ left: `${x(pct)}%` }}
                  />
                ))}
                {row.second !== null ? (
                  <span
                    className="absolute top-1/2 size-3 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-accent bg-ink-1 ring-2 ring-ink-1"
                    style={{ left: `${x(row.second)}%` }}
                  />
                ) : null}
              </span>
              <span className="sr-only">
                : runs at {row.result.runs.map((r) => fmtInt(r.rps)).join(", ")} requests a second
                {row.secondRps && row.second !== null
                  ? `; the repeat at ${fmtInt(row.secondRps)}, ${fmtPct(row.second)} against the median`
                  : ""}
                .
              </span>
            </li>
          ))}
        </ul>
        <div aria-hidden className={`grid ${grid}`}>
          <span className="hidden md:block" />
          <span className="relative block h-6 border-t border-line-2">
            {ticks.map((t, i) => (
              <span
                key={t}
                className={`absolute top-2 font-mono text-[0.75rem] whitespace-nowrap text-fg-3 tabular-nums ${
                  i === 0 ? "" : i === ticks.length - 1 ? "-translate-x-full" : "-translate-x-1/2"
                }`}
                style={{ left: `${x(t)}%` }}
              >
                {t > 0 ? `+${t}` : t < 0 ? `−${Math.abs(t)}` : "0"}
                {i === ticks.length - 1 ? "%" : ""}
              </span>
            ))}
          </span>
        </div>
      </div>
    </figure>
  );
}
