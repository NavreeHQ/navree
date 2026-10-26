import Link from "next/link";

import { RankedBars, type BarRow } from "@/components/bench/ranked-bars";
import { ArrowRightIcon } from "@/components/icons";
import { bench, fmtCompact, fmtInt, fmtMb, fmtMs, highlighted, ordinal, rankIn, rankLowest, rps } from "@/lib/bench";
import type { TestKey } from "@/lib/bench-types";

const FEATURED: TestKey = "deals_list";

/**
 * The landing page's benchmark section: the realistic endpoint as a chart,
 * and the highlighted framework's place in every test, all from the data.
 */
export function BenchTeaser() {
  const navree = highlighted();
  const featured = bench.tests.find((t) => t.key === FEATURED)!;
  const rows: BarRow[] = bench.frameworks
    .filter((f) => f.results[FEATURED])
    .sort((a, b) => rps(b, FEATURED) - rps(a, FEATURED))
    .map((f) => {
      const r = f.results[FEATURED]!;
      return {
        id: f.name,
        label: f.label,
        note: f.category === "micro" ? "micro" : undefined,
        value: r.rps,
        display: fmtCompact(r.rps),
        highlight: f.name === bench.highlight,
        details: [
          ["Requests per second", fmtInt(r.rps)],
          ["p99 latency", `${fmtMs(r.latencyMs.p99)} ms`],
          ["Peak memory", fmtMb(r.peakMemMb)],
        ],
      };
    });

  // The test where the highlighted framework places worst (the first on a tie).
  const worst = bench.tests.reduce((a, b) => (rankIn(b.key).rank > rankIn(a.key).rank ? b : a));
  const peak = rankLowest((f) => f.peakMemMb);
  const startup = rankLowest((f) => f.startupMs);

  return (
    <div className="grid gap-12 lg:grid-cols-[minmax(0,1fr)_20rem] lg:gap-16">
      <figure className="min-w-0">
        <figcaption className="mb-5">
          <span className="block font-semibold text-fg">{featured.label}, requests per second</span>
          <span className="mt-1 block text-sm text-fg-3">{featured.what}</span>
        </figcaption>
        <RankedBars rows={rows} caption={`${featured.label}: requests per second, fastest first`} format="rps" />
      </figure>

      <div>
        <h3 className="font-semibold text-fg">{navree.label} in every test</h3>
        <table className="mt-4 w-full text-sm">
          <caption className="sr-only">
            {navree.label}: requests per second and rank among {bench.frameworks.length} frameworks in each test
          </caption>
          <tbody>
            {bench.tests.map((t) => {
              const rank = rankIn(t.key);
              return (
                <tr key={t.key} className="border-b border-line">
                  <th scope="row" className="py-2.5 pr-3 text-left font-normal text-fg-2">
                    {t.label}
                  </th>
                  <td className="py-2.5 text-right text-fg numeric">{fmtCompact(rps(navree, t.key))}</td>
                  <td className="py-2.5 pl-3 text-right text-fg-3 numeric">
                    {ordinal(rank.rank)} of {rank.of}
                  </td>
                </tr>
              );
            })}
            <tr className="border-b border-line">
              <th scope="row" className="py-2.5 pr-3 text-left font-normal text-fg-2">
                Peak memory
              </th>
              <td className="py-2.5 text-right text-fg numeric">{fmtMb(navree.peakMemMb)}</td>
              <td className="py-2.5 pl-3 text-right text-fg-3 numeric">
                {ordinal(peak.rank)} lowest
              </td>
            </tr>
            <tr className="border-b border-line">
              <th scope="row" className="py-2.5 pr-3 text-left font-normal text-fg-2">
                Ready after
              </th>
              <td className="py-2.5 text-right text-fg numeric">{fmtInt(navree.startupMs)} ms</td>
              <td className="py-2.5 pl-3 text-right text-fg-3 numeric">
                {ordinal(startup.rank)} fastest
              </td>
            </tr>
          </tbody>
        </table>
        <p className="mt-5 text-sm leading-relaxed text-fg-2">
          {worst.label} is where {navree.label} ranks lowest
          {worst.key === "json" || worst.key === "db"
            ? ": with little database work, the framework's own per-request cost is most of what is measured."
            : "."}
        </p>
        <Link href="/benchmarks/" className="link mt-5 inline-flex items-center gap-1.5 font-medium no-underline!">
          See every result, the method and the caveats
          <ArrowRightIcon className="size-4" />
        </Link>
      </div>
    </div>
  );
}
