"use client";

import { useState, type ReactNode } from "react";

import type { TestKey } from "@/lib/bench-types";
import { fmtInt, fmtMb, fmtMs } from "@/lib/format";

export type TableFramework = {
  name: string;
  label: string;
  language: string;
  category: "full-stack" | "micro";
  results: Partial<Record<TestKey, { rps: number; p99: number; errors: number }>>;
  startupMs: number;
  idleMemMb: number;
  peakMemMb: number;
  imageMb: number;
};

type SortKey = TestKey | "startupMs" | "idleMemMb" | "peakMemMb" | "imageMb" | "label";
type Sort = { key: SortKey; dir: "asc" | "desc" } | null;

/** A sortable column heading: a button inside the `th`, with `aria-sort`. */
function Header({
  k,
  first,
  sort,
  onToggle,
  children,
  className = "",
}: {
  k: SortKey;
  first: "asc" | "desc";
  sort: Sort;
  onToggle: (key: SortKey, first: "asc" | "desc") => void;
  children: ReactNode;
  className?: string;
}) {
  const active = sort?.key === k;
  const dir = active ? sort.dir : null;
  return (
    <th scope="col" aria-sort={dir === "asc" ? "ascending" : dir === "desc" ? "descending" : "none"} className={className}>
      <button type="button" onClick={() => onToggle(k, first)} className="results-sort">
        {children}
        <span aria-hidden className={active ? "results-sort-icon is-active" : "results-sort-icon"}>
          {dir === "asc" ? "↑" : dir === "desc" ? "↓" : "↕"}
        </span>
      </button>
    </th>
  );
}

/** Every number in one table. Click a column heading to sort by it. */
export function ResultsTable({
  frameworks,
  tests,
  highlight,
}: {
  frameworks: TableFramework[];
  tests: { key: TestKey; label: string }[];
  highlight: string;
}) {
  const [sort, setSort] = useState<Sort>(null);

  const value = (f: TableFramework, key: SortKey): number | string => {
    if (key === "label") return f.label.toLowerCase();
    if (key === "startupMs" || key === "idleMemMb" || key === "peakMemMb" || key === "imageMb") return f[key];
    return f.results[key]?.rps ?? -1;
  };

  const rows = sort
    ? [...frameworks].sort((a, b) => {
        const x = value(a, sort.key);
        const y = value(b, sort.key);
        const order = x < y ? -1 : x > y ? 1 : 0;
        return sort.dir === "asc" ? order : -order;
      })
    : frameworks;

  function toggle(key: SortKey, first: "asc" | "desc") {
    setSort((current) => (current?.key === key ? { key, dir: current.dir === "asc" ? "desc" : "asc" } : { key, dir: first }));
  }

  const header = { sort, onToggle: toggle };

  return (
    <div className="results-wrap" role="region" aria-label="All results" tabIndex={0}>
      <table className="results-table">
        <caption className="sr-only">
          Requests per second (the median of the measured runs) and p99 latency for every test, with startup time, memory
          and image size. The column headings sort the table.
        </caption>
        <thead>
          <tr>
            <Header {...header} k="label" first="asc" className="results-sticky">
              Framework
            </Header>
            {tests.map((t) => (
              <Header {...header} key={t.key} k={t.key} first="desc">
                {t.label}
              </Header>
            ))}
            <Header {...header} k="startupMs" first="asc">
              Startup
            </Header>
            <Header {...header} k="idleMemMb" first="asc">
              Idle memory
            </Header>
            <Header {...header} k="peakMemMb" first="asc">
              Peak memory
            </Header>
            <Header {...header} k="imageMb" first="asc">
              Image
            </Header>
          </tr>
        </thead>
        <tbody>
          {rows.map((f) => (
            <tr key={f.name} className={f.name === highlight ? "is-highlight" : undefined}>
              <th scope="row" className="results-sticky">
                <span className="block font-medium text-fg">{f.label}</span>
                <span className="block text-xs font-normal text-fg-3">
                  {f.language}
                  {f.category === "micro" ? ", micro" : ""}
                </span>
              </th>
              {tests.map((t) => {
                const r = f.results[t.key];
                return (
                  <td key={t.key}>
                    {r ? (
                      <>
                        <span className="block text-fg-1">{fmtInt(r.rps)}</span>
                        <span className="block text-xs text-fg-3">{fmtMs(r.p99)} ms p99</span>
                        {r.errors ? <span className="block text-xs text-amber">{fmtInt(r.errors)} errors</span> : null}
                      </>
                    ) : (
                      <span className="text-fg-3">n/a</span>
                    )}
                  </td>
                );
              })}
              <td>{fmtInt(f.startupMs)} ms</td>
              <td>{fmtMb(f.idleMemMb)}</td>
              <td>{fmtMb(f.peakMemMb)}</td>
              <td>{fmtMb(f.imageMb)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
