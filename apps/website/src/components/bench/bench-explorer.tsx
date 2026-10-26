"use client";

import { useId, useState } from "react";

import type { TestKey } from "@/lib/bench-types";
import { fmtCompact, fmtInt, fmtMs } from "@/lib/format";

import { RankedBars, type BarRow } from "./ranked-bars";

export type ExplorerFramework = {
  name: string;
  label: string;
  language: string;
  category: "full-stack" | "micro";
  results: Partial<
    Record<
      TestKey,
      { rps: number; p50: number; p90: number; p99: number; cpu: number; pgCpu: number; wrkCpu: number; errors: number }
    >
  >;
};

type Scope = "all" | "full-stack";

/**
 * The results explorer: pick a test (and optionally full-stack frameworks
 * only); both charts below follow the selection.
 */
export function BenchExplorer({
  frameworks,
  tests,
  highlight,
  appCpus,
  pgCpus,
  loadCpus,
}: {
  frameworks: ExplorerFramework[];
  tests: { key: TestKey; label: string; path: string; what: string }[];
  highlight: string;
  appCpus: number;
  pgCpus: number;
  loadCpus: number;
}) {
  const [test, setTest] = useState<TestKey>(tests[0].key);
  const [scope, setScope] = useState<Scope>("all");
  const name = useId();
  const info = tests.find((t) => t.key === test) ?? tests[0];

  const visible = frameworks.filter((f) => f.results[test] && (scope === "all" || f.category === "full-stack"));

  const label = (f: ExplorerFramework) => f.label;
  const note = (f: ExplorerFramework) => (f.category === "micro" ? "micro" : undefined);

  const throughput: BarRow[] = [...visible]
    .sort((a, b) => b.results[test]!.rps - a.results[test]!.rps)
    .map((f) => {
      const r = f.results[test]!;
      return {
        id: f.name,
        label: label(f),
        note: note(f),
        value: r.rps,
        display: fmtCompact(r.rps),
        highlight: f.name === highlight,
        details: [
          ["Requests per second", fmtInt(r.rps)],
          ["p99 latency", `${fmtMs(r.p99)} ms`],
          ["App CPU", `${fmtInt(r.cpu)}% of ${fmtInt(appCpus * 100)}%`],
          ["PostgreSQL CPU", `${fmtInt(r.pgCpu)}% of ${fmtInt(pgCpus * 100)}%`],
          ["wrk CPU", `${fmtInt(r.wrkCpu)}% of ${fmtInt(loadCpus * 100)}%`],
          ["Errors", fmtInt(r.errors)],
        ],
      };
    });

  const latency: BarRow[] = [...visible]
    .sort((a, b) => a.results[test]!.p99 - b.results[test]!.p99)
    .map((f) => {
      const r = f.results[test]!;
      return {
        id: f.name,
        label: label(f),
        note: note(f),
        value: r.p99,
        display: `${fmtMs(r.p99)} ms`,
        highlight: f.name === highlight,
        details: [
          ["p50", `${fmtMs(r.p50)} ms`],
          ["p90", `${fmtMs(r.p90)} ms`],
          ["p99", `${fmtMs(r.p99)} ms`],
          ["Requests per second", fmtInt(r.rps)],
        ],
      };
    });

  return (
    <div>
      <div className="sticky top-(--header-h) z-20 -mx-4 border-b border-line bg-ink-0/90 px-4 py-3 backdrop-blur-md sm:-mx-6 sm:px-6 lg:mx-0 lg:rounded-b-xl lg:px-0">
        <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
          <fieldset className="min-w-0">
            <legend className="sr-only">Test</legend>
            <div className="segmented">
              {tests.map((t) => (
                <label key={t.key} className="segmented-item">
                  <input
                    type="radio"
                    name={`${name}-test`}
                    value={t.key}
                    checked={test === t.key}
                    onChange={() => setTest(t.key)}
                    className="sr-only"
                  />
                  <span>{t.label}</span>
                </label>
              ))}
            </div>
          </fieldset>
          <fieldset>
            <legend className="sr-only">Frameworks</legend>
            <div className="segmented">
              {(
                [
                  ["all", "All"],
                  ["full-stack", "Full-stack only"],
                ] as const
              ).map(([value, text]) => (
                <label key={value} className="segmented-item">
                  <input
                    type="radio"
                    name={`${name}-scope`}
                    value={value}
                    checked={scope === value}
                    onChange={() => setScope(value)}
                    className="sr-only"
                  />
                  <span>{text}</span>
                </label>
              ))}
            </div>
          </fieldset>
        </div>
      </div>

      <p className="mt-6 text-[0.9375rem] leading-relaxed text-fg-2">
        <code className="inline-code mr-2">{info.path}</code>
        {info.what}
      </p>

      <section aria-labelledby="results-throughput" className="mt-8">
        <h3 id="results-throughput" className="text-lg font-semibold text-fg">
          Requests per second
          <span className="ml-2 text-sm font-normal text-fg-3">higher is better</span>
        </h3>
        <div className="mt-5">
          <RankedBars rows={throughput} caption={`${info.label}: requests per second, fastest first`} format="rps" />
        </div>
      </section>

      <section aria-labelledby="latency" className="mt-14 scroll-mt-20">
        <h3 id="latency" className="text-lg font-semibold text-fg">
          p99 latency
          <span className="ml-2 text-sm font-normal text-fg-3">lower is better</span>
        </h3>
        <p className="mt-1.5 text-[0.9375rem] text-fg-2">
          99 of every 100 requests in the median run finished within this time, under the same load.
        </p>
        <div className="mt-5">
          <RankedBars rows={latency} caption={`${info.label}: p99 latency, lowest first`} format="ms" />
        </div>
      </section>
    </div>
  );
}
