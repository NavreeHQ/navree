import type { Metadata } from "next";
import Link from "next/link";
import type { ReactNode } from "react";

import { BenchExplorer, type ExplorerFramework } from "@/components/bench/bench-explorer";
import { RankedBars, type BarRow } from "@/components/bench/ranked-bars";
import { ResultsTable, type TableFramework } from "@/components/bench/results-table";
import { StackDetails } from "@/components/bench/stack-details";
import { Toc } from "@/components/docs/toc";
import { DownloadIcon, ExternalIcon } from "@/components/icons";
import { CodeBlock } from "@/components/mdx/code-block";
import {
  bench,
  byName,
  fmtCompact,
  fmtInt,
  fmtMb,
  fmtPct,
  fmtTimes,
  highlighted,
  measuredOn,
  ordinal,
  rankIn,
  rankLowest,
  rps,
  testInfo,
} from "@/lib/bench";
import type { Framework, TestKey } from "@/lib/bench-types";
import { highlight } from "@/lib/mdx";
import { repoBlob, repoTree, site } from "@site";

export const metadata: Metadata = {
  title: "Benchmarks",
  description: `${site.name} against ${bench.frameworks.length - 1} other web frameworks on the same API and the same machine: throughput, latency, memory, startup, and where ${site.name} is slower.`,
  alternates: { canonical: "/benchmarks/" },
};

const headings = [
  { depth: 2 as const, id: "results", text: "Results" },
  { depth: 3 as const, id: "latency", text: "Latency" },
  { depth: 2 as const, id: "memory-and-startup", text: "Memory and startup" },
  { depth: 2 as const, id: "all-results", text: "All results" },
  { depth: 2 as const, id: "where-navree-is-slower", text: `Where ${site.name} is slower` },
  { depth: 2 as const, id: "methodology", text: "Methodology" },
  { depth: 2 as const, id: "caveats", text: "Caveats" },
  { depth: 2 as const, id: "stacks", text: "The stacks" },
  { depth: 2 as const, id: "reproduce", text: "Reproduce it yourself" },
];

function Section({ id, title, lead, children }: { id: string; title: string; lead?: ReactNode; children: ReactNode }) {
  return (
    <section aria-labelledby={id} className="mt-20 first:mt-0">
      <h2 id={id} className="text-[1.75rem] leading-tight font-semibold tracking-[-0.025em] text-fg">
        <a href={`#${id}`} className="heading-anchor">
          {title}
        </a>
      </h2>
      {lead ? <div className="mt-3 max-w-[44rem] text-[1.0625rem] leading-relaxed text-fg-2">{lead}</div> : null}
      <div className="mt-8">{children}</div>
    </section>
  );
}

function Prose({ children }: { children: ReactNode }) {
  return <div className="prose-navree max-w-[44rem]">{children}</div>;
}

/** "A, B and C". */
function list(items: string[]): string {
  if (items.length <= 1) return items.join("");
  return `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

function minutesBetween(a: string, b: string): number {
  return Math.round((new Date(b).getTime() - new Date(a).getTime()) / 60000);
}

export default async function BenchmarksPage() {
  const navree = highlighted();
  const name = site.name;
  const { settings } = bench;
  const others = bench.frameworks.length - 1;
  const checks = navree.conformance.passed ?? 0;
  const allPassed = bench.frameworks.every((f) => f.conformance.ok && !f.conformance.failed);
  const fullStackCount = bench.frameworks.filter((f) => f.category === "full-stack").length;
  const label = (key: TestKey) => testInfo(key).label;

  // ------------------------------------------------------------ findings
  const listRank = rankIn("deals_list", "full-stack");
  const createRank = rankIn("deals_create", "full-stack");
  const jsonRank = rankIn("json");
  const dbRank = rankIn("db");
  const peakRank = rankLowest((f) => f.peakMemMb);
  const startupRank = rankLowest((f) => f.startupMs);
  const axum = byName("axum");
  const loco = byName("loco");
  const behindOnList = ["nestjs-fastify", "rails", "django", "laravel-octane"]
    .map(byName)
    .filter((f): f is Framework => Boolean(f) && rps(f!, "deals_list") < rps(navree, "deals_list"));
  const aheadOnList = bench.frameworks.filter(
    (f) => f.category === "full-stack" && rps(f, "deals_list") > rps(navree, "deals_list"),
  ).sort((a, b) => rps(b, "deals_list") - rps(a, "deals_list"));
  const totalErrors =Object.values(navree.results).reduce((sum, r) => sum + (r?.errors ?? 0), 0);
  const jsonCpu = navree.results.json?.cpuPct ?? 0;
  const dbCpu = navree.results.db?.cpuPct ?? 0;
  const dbPg = navree.results.db?.pgCpuPct ?? 0;
  const appCap = settings.app.count * 100;
  const pgCap = settings.pg.count * 100;
  const loadCap = settings.load.count * 100;

  // ------------------------------------------------------------- caveats
  const loadLimited = bench.frameworks
    .filter((f) => (f.results.json?.wrkCpuPct ?? 0) >= loadCap * 0.75)
    .sort((a, b) => (b.results.json?.wrkCpuPct ?? 0) - (a.results.json?.wrkCpuPct ?? 0));
  const dbBound = (key: TestKey) => {
    const busy = bench.frameworks.filter((f) => (f.results[key]?.pgCpuPct ?? 0) >= pgCap * 0.6);
    const max = Math.max(...bench.frameworks.map((f) => f.results[key]?.pgCpuPct ?? 0));
    return { busy: busy.length, max };
  };
  const queriesBound = dbBound("queries");
  const listBound = dbBound("deals_list");
  const queryLeaders = [...bench.frameworks].sort((a, b) => rps(b, "queries") - rps(a, "queries")).slice(0, 2);
  const fpm = bench.fpmDefault;
  const fpmRerun = byName("laravel-fpm");
  const drift = bench.drift;
  const maxDrift = drift ? Math.max(...drift.tests.map((t) => Math.abs(t.deltaPct))) : 0;

  // -------------------------------------------------------------- charts
  const explorer: ExplorerFramework[] = bench.frameworks.map((f) => ({
    name: f.name,
    label: f.label,
    language: f.language,
    category: f.category,
    results: Object.fromEntries(
      Object.entries(f.results).map(([key, r]) => [
        key,
        {
          rps: r.rps,
          p50: r.latencyMs.p50,
          p90: r.latencyMs.p90,
          p99: r.latencyMs.p99,
          cpu: r.cpuPct,
          pgCpu: r.pgCpuPct,
          wrkCpu: r.wrkCpuPct,
          errors: r.errors,
        },
      ]),
    ),
  }));

  const resourceRows = (metric: (f: Framework) => number, display: (f: Framework) => string): BarRow[] =>
    [...bench.frameworks]
      .sort((a, b) => metric(a) - metric(b))
      .map((f) => ({
        id: f.name,
        label: f.label,
        note: f.category === "micro" ? "micro" : undefined,
        value: metric(f),
        display: display(f),
        highlight: f.name === bench.highlight,
        details: [
          ["Peak memory under load", fmtMb(f.peakMemMb)],
          ["Idle memory", fmtMb(f.idleMemMb)],
          ["Ready after", `${fmtInt(f.startupMs)} ms`],
          ["Image size", fmtMb(f.imageMb)],
        ],
      }));

  const table: TableFramework[] = bench.frameworks.map((f) => ({
    name: f.name,
    label: f.label,
    language: f.language,
    category: f.category,
    results: Object.fromEntries(
      Object.entries(f.results).map(([key, r]) => [key, { rps: r.rps, p99: r.latencyMs.p99, errors: r.errors }]),
    ),
    startupMs: f.startupMs,
    idleMemMb: f.idleMemMb,
    peakMemMb: f.peakMemMb,
    imageMb: f.imageMb,
  }));

  const repoName = site.repoUrl.replace(/\/$/, "").split("/").pop() ?? "navree";
  const reproduce = [
    `git clone ${site.repoUrl}`,
    `cd ${repoName}`,
    `node bench/run.mjs --smoke --only ${navree.name}   # build, conformance, then 5 s per test`,
    `node bench/run.mjs                        # the full run, one framework at a time`,
    `node bench/run.mjs --only ${navree.name},axum      # or just some of them`,
    `node bench/report.mjs                     # writes bench/results/report.html`,
  ].join("\n");
  const reproduceHtml = await highlight(reproduce, "shellscript");
  const preInner = reproduceHtml.replace(/^<pre[^>]*>/, "").replace(/<\/pre>$/, "");

  return (
    <div className="mx-auto w-full max-w-[90rem] px-4 sm:px-6 lg:px-8 xl:grid xl:grid-cols-[minmax(0,1fr)_13.5rem] xl:gap-16">
      <div className="min-w-0 max-w-[64rem] pt-12 pb-24 lg:pt-16">
        {/* ------------------------------------------------------------ header */}
        <header>
          <h1 className="text-[2.75rem] leading-[1.05] font-semibold tracking-[-0.04em] text-fg sm:text-[3.5rem]">Benchmarks</h1>
          <p className="mt-5 max-w-[46rem] text-lg leading-relaxed text-pretty text-fg-1 sm:text-xl">
            {name} and {others} other web frameworks implement the same small API. Each one passed the same {checks}-check
            conformance test, then ran alone on the same machine, on the same CPUs, against the same PostgreSQL.
          </p>
          <p className="mt-4 text-sm text-fg-3">
            Measured on {measuredOn()}. Every number on this page is read from the{" "}
            <a href="#reproduce" className="link">
              raw results
            </a>{" "}
            when the site is built.
          </p>
        </header>

        {/* --------------------------------------------------------- findings */}
        <section aria-labelledby="findings" className="mt-12 rounded-2xl border border-line bg-ink-1 p-6 sm:p-8">
          <h2 id="findings" className="text-lg font-semibold text-fg">
            What the numbers say about {name}
          </h2>
          <ul className="mt-5 space-y-4 text-[0.9375rem] leading-relaxed text-fg-1">
            <li className="finding">
              <strong className="text-fg">
                On the realistic endpoints, {name} is{" "}
                {listRank.rank === createRank.rank
                  ? `${ordinal(listRank.rank)} of the ${fullStackCount} full-stack frameworks on both`
                  : `${ordinal(listRank.rank)} and ${ordinal(createRank.rank)} of the ${fullStackCount} full-stack frameworks`}
                .
              </strong>{" "}
              It serves {fmtInt(rps(navree, "deals_list"))} authenticated, paginated lists a second, behind{" "}
              {list(aheadOnList.map((f) => `${f.label} (${fmtInt(rps(f, "deals_list"))})`))}.
              {behindOnList.length
                ? ` That is ${list(behindOnList.map((f) => `${fmtTimes(rps(navree, "deals_list") / rps(f, "deals_list"))} ${f.label}`))}.`
                : ""}
            </li>
            <li className="finding">
              <strong className="text-fg">Its own per-request cost is high.</strong> On plain JSON it is{" "}
              {ordinal(jsonRank.rank)} of {jsonRank.of}, at {fmtCompact(rps(navree, "json"))} requests a second
              {axum ? `, against ${fmtCompact(rps(axum, "json"))} for Axum, the router it is built on` : ""}
              {loco ? `, and ${fmtCompact(rps(loco, "json"))} for Loco` : ""}. On one query it is {ordinal(dbRank.rank)}{" "}
              of {dbRank.of}. <Link href="#where-navree-is-slower" className="link">Here is why.</Link>
            </li>
            <li className="finding">
              <strong className="text-fg">Its footprint is tiny.</strong> It idles at {fmtMb(navree.idleMemMb)}, peaks at{" "}
              {fmtMb(navree.peakMemMb)} under load ({ordinal(peakRank.rank)} lowest of {peakRank.of}), is ready{" "}
              {fmtInt(navree.startupMs)} ms after it starts, and returned {totalErrors === 0 ? "no errors" : `${fmtInt(totalErrors)} errors`} in
              any test.
            </li>
          </ul>
        </section>

        {/* ---------------------------------------------------------- results */}
        <div className="mt-20">
          <Section
            id="results"
            title="Results"
            lead={
              <p>
                Pick a test. Bars show the median of {settings.runs} measured runs; hover or focus a bar for its latency
                and CPU use. <span className="text-fg-3">micro</span> marks micro-frameworks, measured with their most
                common libraries instead of a full stack.
              </p>
            }
          >
            <BenchExplorer
              frameworks={explorer}
              tests={bench.tests}
              highlight={bench.highlight}
              appCpus={settings.app.count}
              pgCpus={settings.pg.count}
              loadCpus={settings.load.count}
            />
          </Section>
        </div>

        <Section
          id="memory-and-startup"
          title="Memory and startup"
          lead={
            <p>
              Peak memory is the most the app&apos;s container used during any test. Startup is the time from starting the
              container to the first successful response.
            </p>
          }
        >
          <h3 className="text-lg font-semibold text-fg">
            Peak memory under load <span className="ml-2 text-sm font-normal text-fg-3">lower is better</span>
          </h3>
          <div className="mt-5">
            <RankedBars rows={resourceRows((f) => f.peakMemMb, (f) => fmtMb(f.peakMemMb))} caption="Peak memory under load, lowest first" format="mb" />
          </div>
          <h3 className="mt-14 text-lg font-semibold text-fg">
            Idle memory <span className="ml-2 text-sm font-normal text-fg-3">lower is better</span>
          </h3>
          <div className="mt-5">
            <RankedBars rows={resourceRows((f) => f.idleMemMb, (f) => fmtMb(f.idleMemMb))} caption="Idle memory, lowest first" format="mb" />
          </div>
          <h3 className="mt-14 text-lg font-semibold text-fg">
            Startup time <span className="ml-2 text-sm font-normal text-fg-3">lower is better</span>
          </h3>
          <div className="mt-5">
            <RankedBars
              rows={resourceRows((f) => f.startupMs, (f) => `${fmtInt(f.startupMs)} ms`)}
              caption="Startup time, fastest first"
              format="ms"
            />
          </div>
          <p className="mt-4 text-sm text-fg-3">
            {name} was ready in {fmtInt(navree.startupMs)} ms, {ordinal(startupRank.rank)} of {startupRank.of}.
          </p>
        </Section>

        <Section
          id="all-results"
          title="All results"
          lead={<p>Requests per second and p99 latency for every test, with startup, memory and image size. Sort by any column.</p>}
        >
          <ResultsTable frameworks={table} tests={bench.tests} highlight={bench.highlight} />
        </Section>

        {/* --------------------------------------------------------- candour */}
        <Section
          id="where-navree-is-slower"
          title={`Where ${name} is slower`}
          lead={
            <p>
              {name} does more work per request than the frameworks it is compared with, and some of that work is waste.
              The gap is largest where the framework is all there is to measure.
            </p>
          }
        >
          {axum && loco ? (
            <div className="table-wrap mt-0 max-w-[44rem]" role="region" aria-label={`${name} against Axum and Loco`} tabIndex={0}>
              <table className="numeric">
                <caption className="sr-only">Requests per second of {name}, Axum and Loco, and {name} as a share of each</caption>
                <thead>
                  <tr>
                    <th scope="col">Test</th>
                    <th scope="col" className="text-right">
                      {name}
                    </th>
                    <th scope="col" className="text-right">
                      Axum + sqlx
                    </th>
                    <th scope="col" className="text-right">
                      Loco
                    </th>
                    <th scope="col" className="text-right">
                      {name} as a share
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {bench.tests.map((t) => (
                    <tr key={t.key}>
                      <th scope="row" className="font-normal text-fg-1">
                        {t.label}
                      </th>
                      <td className="text-right text-fg">{fmtCompact(rps(navree, t.key))}</td>
                      <td className="text-right">{fmtCompact(rps(axum, t.key))}</td>
                      <td className="text-right">{fmtCompact(rps(loco, t.key))}</td>
                      <td className="text-right text-fg-2">
                        {Math.round((rps(navree, t.key) / rps(axum, t.key)) * 100)}% of Axum,{" "}
                        {Math.round((rps(navree, t.key) / rps(loco, t.key)) * 100)}% of Loco
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : null}
          <Prose>
            <p>
              All three run on Axum, Tokio and sqlx, and {name} and Loco both add SeaORM, so the difference is the framework.
              The time goes into {name}&apos;s request pipeline, not the database: on JSON, {name} used {fmtInt(jsonCpu)}% of
              its {fmtInt(appCap)}% of CPU, and on one query {fmtInt(dbCpu)}% while PostgreSQL used only {fmtInt(dbPg)}% of
              its {fmtInt(pgCap)}%.
            </p>
            <p>Reading the code at the time of the run shows where the time goes. Fixing these is the next piece of work:</p>
            <ol>
              <li>
                <code>navree-db</code> leaves sqlx&apos;s <code>test_before_acquire</code> on, so every pool checkout pings
                PostgreSQL first: <code>/db</code> costs two round trips instead of one.
              </li>
              <li>
                Every unit-of-work transaction adds a <code>SELECT set_config(&apos;navree.actor&apos;, …)</code> round trip.
              </li>
              <li>
                The unit-of-work layer spawns a task for every request, <code>/json</code> included.
              </li>
              <li>
                Process-wide locks are taken on every request: maintenance mode (a mutex and a read-write lock) and the
                per-IP rate limiter (one global <code>Mutex&lt;HashMap&gt;</code>).
              </li>
              <li>
                The request-context layer builds metrics labels (fresh strings) even when no metrics recorder is installed,
                plus a request scope and a UUID request id.
              </li>
              <li>
                <code>CurrentUser</code> finds <code>AuthService</code> by scanning every provider, and loads every user
                column, including the password hash and the two-factor secrets.
              </li>
              <li>
                JSON bodies are parsed twice: into a <code>serde_json::Value</code>, then into the DTO.
              </li>
              <li>
                <code>Repo::insert</code> serializes each new row for a change event even when nothing listens for change
                events.
              </li>
            </ol>
            <p>
              What this means for an application: once a request does real database work, the gap narrows. On the
              authenticated list, {name} delivers {Math.round((rps(navree, "deals_list") / rps(loco ?? navree, "deals_list")) * 100)}%
              of Loco&apos;s throughput{axum ? ` and ${Math.round((rps(navree, "deals_list") / rps(axum, "deals_list")) * 100)}% of hand-written Axum` : ""}.
            </p>
          </Prose>
        </Section>

        {/* ------------------------------------------------------ methodology */}
        <Section id="methodology" title="Methodology">
          <Prose>
            <h3>One contract</h3>
            <p>
              Every implementation serves the same five endpoints, defined in{" "}
              <a href={repoBlob("bench/SPEC.md")}>bench/SPEC.md</a>, from the same seeded PostgreSQL database:
            </p>
            <ul>
              {bench.tests.map((t) => (
                <li key={t.key}>
                  <strong>{t.label}</strong> (<code>{t.path}</code>): {t.what.charAt(0).toLowerCase() + t.what.slice(1)}
                </li>
              ))}
            </ul>
            <p>
              The two <code>/api/deals</code> endpoints are what a typical API endpoint does: verify an HS256 JWT, load the
              user, validate the input, query or insert, and serialize the result.
            </p>

            <h3>A conformance gate</h3>
            <p>
              Before it is measured, each app must pass {checks} checks in{" "}
              <a href={repoBlob("bench/tools/conformance.mjs")}>bench/tools/conformance.mjs</a>: response shapes and
              types, the clamping of <code>/queries</code>, every way a token can be wrong, pagination order and edge
              cases, and validation failures. An app that fails is not benchmarked.{" "}
              {allPassed ? `All ${bench.frameworks.length} passed.` : ""}
            </p>

            <h3>Fairness rules</h3>
            <ul>
              <li>Release builds and production settings, with no per-request access logging.</li>
              <li>
                Every app uses all {settings.app.count} of its CPUs the way its framework recommends: one multi-threaded
                process, or one worker per CPU.
              </li>
              <li>At most 64 database connections in total, across all processes.</li>
              <li>
                Idiomatic stacks: full-stack frameworks use their own ORM, validation, auth and serializers; micro-frameworks
                use their most common companions.
              </li>
              <li>No caching of any kind: every request reads the database.</li>
              <li>The latest stable versions, pinned in lockfiles, on Debian-based images.</li>
            </ul>

            <h3>CPU pinning and timing</h3>
            <p>
              The machine has {settings.ncpu} logical CPUs, split so that nothing competes: the app gets{" "}
              {settings.app.count} (<code>{settings.app.range}</code>), PostgreSQL {settings.pg.count} (
              <code>{settings.pg.range}</code>) and the load generator, wrk, {settings.load.count} (
              <code>{settings.load.range}</code>). Only one app runs at a time, and its database is recreated from the same
              template before it starts.
            </p>
            <p>
              Each test runs wrk with {settings.threads} threads and {settings.connections} connections: a{" "}
              {settings.warmupS}-second warmup, then {settings.runs} measured runs of {settings.durationS} seconds. The page
              reports the median run; memory and CPU are sampled while the test runs.
            </p>

            {drift ? (
              <>
                <h3>The drift check</h3>
                <p>
                  A laptop can heat up and slow down over a long run, so {name} was measured first and again after every
                  other framework, {minutesBetween(drift.firstAt, drift.secondAt)} minutes later. Every test landed within{" "}
                  {maxDrift.toFixed(1)}% of the first run, so the machine did not drift.
                </p>
              </>
            ) : null}
          </Prose>
          {drift ? (
            <div className="table-wrap max-w-[44rem]" role="region" aria-label="Drift check" tabIndex={0}>
              <table className="numeric">
                <caption className="sr-only">{name}: requests per second in the first run and in the control run at the end</caption>
                <thead>
                  <tr>
                    <th scope="col">Test</th>
                    <th scope="col" className="text-right">
                      First run
                    </th>
                    <th scope="col" className="text-right">
                      At the end
                    </th>
                    <th scope="col" className="text-right">
                      Change
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {drift.tests.map((t) => (
                    <tr key={t.key}>
                      <th scope="row" className="font-normal text-fg-1">
                        {label(t.key)}
                      </th>
                      <td className="text-right">{fmtInt(t.first)}</td>
                      <td className="text-right">{fmtInt(t.second)}</td>
                      <td className="text-right text-fg">{fmtPct(t.deltaPct)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : null}
        </Section>

        {/* ---------------------------------------------------------- caveats */}
        <Section id="caveats" title="Caveats">
          <Prose>
            <h3>The load generator limits the fastest JSON results</h3>
            <p>
              On JSON, wrk itself was close to its {fmtInt(loadCap)}% of CPU for{" "}
              {list(loadLimited.map((f) => `${f.label} (${fmtInt(f.results.json?.wrkCpuPct ?? 0)}%)`))}, so their real
              ceilings are higher than shown. Every other result is limited by the app or by PostgreSQL.
            </p>

            <h3>Some tests are bound by the database</h3>
            <p>
              On {label("queries")}, PostgreSQL used at least 60% of its {fmtInt(pgCap)}% of CPU for {queriesBound.busy} of{" "}
              {bench.frameworks.length} frameworks, and up to {fmtInt(queriesBound.max)}%; on {label("deals_list")},{" "}
              {listBound.busy} of them. That compresses the differences between fast frameworks. Drivers that cache
              prepared statements (pgx in Gin, JDBC in Spring Boot) cost PostgreSQL much less per query; the leaders of{" "}
              {label("queries")} are {list(queryLeaders.map((f) => f.label))}.
            </p>

            {fpm && fpmRerun ? (
              <>
                <h3>Laravel on PHP-FPM ran out of ports</h3>
                <p>
                  By default, Laravel under PHP-FPM opens a new PostgreSQL connection for every request. With Docker&apos;s
                  default kernel settings, the closed connections used up the container&apos;s local ports within seconds:{" "}
                  {list(
                    fpm.tests
                      .filter((t) => t.errors > 0)
                      .map((t) => `${Math.round(t.errorPct)}% of ${label(t.key)} requests failed`),
                  )}
                  . The figures on this page come from a rerun with <code>net.ipv4.tcp_tw_reuse=1</code>, the standard fix,
                  which let the kernel reuse those ports. Production setups usually put PgBouncer in front of PostgreSQL or
                  use persistent connections. <a href={fpm.rawHref}>The failed run is kept</a>.
                </p>
              </>
            ) : null}

            <h3>wrk runs on the monotonic clock</h3>
            <p>
              Stock wrk times requests with the wall clock. Inside the Docker VM the wall clock occasionally stepped
              backwards, and wrk counted phantom timeouts. The wrk used here (<a href={repoTree("bench/wrk")}>bench/wrk</a>)
              is patched to use the monotonic clock.
            </p>

            <h3>These are laptop numbers</h3>
            <p>
              Everything ran on one laptop (Intel Core Ultra 9 275HX, Docker Desktop on WSL2), on one Docker bridge network.
              Absolute figures will differ on servers; the ordering is what to look at.
            </p>

            <h3>Measured under the working name</h3>
            <p>
              {name} was developed and measured under its working name, Keel. The measured code is the same; only the names
              changed.
            </p>
          </Prose>
        </Section>

        {/* ----------------------------------------------------------- stacks */}
        <Section
          id="stacks"
          title="The stacks"
          lead={
            <p>
              What each implementation runs, from its README. The READMEs also record every judgement call, such as{" "}
              {name}&apos;s raised per-IP rate limit (wrk is a single client).
            </p>
          }
        >
          <StackDetails frameworks={bench.frameworks} highlight={bench.highlight} />
        </Section>

        {/* -------------------------------------------------------- reproduce */}
        <Section
          id="reproduce"
          title="Reproduce it yourself"
          lead={
            <p>
              You need Docker, Node.js and a machine with {settings.ncpu} logical CPUs, or change <code className="inline-code">--app-cpus</code>,{" "}
              <code className="inline-code">--pg-cpus</code> and <code className="inline-code">--load-cpus</code> to fit
              yours. The script starts PostgreSQL, builds wrk and every app, and writes one JSON file per framework.
            </p>
          }
        >
          <div className="max-w-[44rem]">
            <CodeBlock data-title="Terminal" data-language="shellscript" className="shiki navree-night">
              <span dangerouslySetInnerHTML={{ __html: preInner }} />
            </CodeBlock>
          </div>

          <div className="mt-12 grid gap-10 md:grid-cols-2">
            <div>
              <h3 className="text-lg font-semibold text-fg">Raw results</h3>
              <p className="mt-2 text-sm leading-relaxed text-fg-2">
                The files this page is built from. <code className="inline-code">extra/</code> holds the drift check and
                the failed Laravel run.
              </p>
              <ul className="mt-4 divide-y divide-line border-y border-line text-sm">
                {bench.downloads.map((d) => (
                  <li key={d.file}>
                    <a href={d.href} download className="flex items-center justify-between gap-4 py-2 text-fg-1 hover:text-fg">
                      <span className="font-mono text-[0.8125rem]">{d.file}</span>
                      <span className="flex items-center gap-2 text-fg-3">
                        {fmtInt(d.bytes / 1024)} KB <DownloadIcon className="size-3.5" />
                      </span>
                    </a>
                  </li>
                ))}
              </ul>
            </div>
            <div>
              <h3 className="text-lg font-semibold text-fg">Source of each implementation</h3>
              <p className="mt-2 text-sm leading-relaxed text-fg-2">
                Each directory has a Dockerfile, the code and a README with versions and judgement calls.
              </p>
              <ul className="mt-4 divide-y divide-line border-y border-line text-sm">
                {bench.frameworks.map((f) => (
                  <li key={f.name}>
                    <a href={repoTree(f.sourceDir)} className="flex items-center justify-between gap-4 py-2 text-fg-1 hover:text-fg">
                      <span>{f.label}</span>
                      <span className="flex items-center gap-2 font-mono text-[0.8125rem] text-fg-3">
                        {f.sourceDir.replace(/^bench\//, "")} <ExternalIcon className="size-3.5" />
                      </span>
                    </a>
                  </li>
                ))}
              </ul>
            </div>
          </div>
        </Section>
      </div>

      <aside className="hidden xl:block">
        <div className="sticky top-[calc(var(--header-h)+3rem)] pt-16">
          <Toc headings={headings} />
        </div>
      </aside>
    </div>
  );
}
