import type { Metadata } from "next";
import Link from "next/link";
import type { ReactNode } from "react";

import { CodePanel, type CodePane } from "@/components/bench/code-panel";
import { CpuChart, appBusiestEverywhere } from "@/components/bench/cpu-chart";
import { LatencyChart } from "@/components/bench/latency-chart";
import { MachineDiagram } from "@/components/bench/machine-diagram";
import { MemoryAndStartup } from "@/components/bench/memory-startup";
import { RepeatChart } from "@/components/bench/repeat-chart";
import { SectionNav } from "@/components/bench/section-nav";
import { ThroughputBoard } from "@/components/bench/throughput-board";
import { TickerNumber } from "@/components/bench/ticker";
import { ArrowRightIcon, DownloadIcon, ExternalIcon } from "@/components/icons";
import { CodeBlock } from "@/components/mdx/code-block";
import { GridPattern } from "@/components/magicui/grid-pattern";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  cpus,
  fmtInt,
  maxRepeatDelta,
  mb,
  measuredOn,
  measuredRuns,
  perf,
  stackVersion,
  totalErrors,
  worstTailRatio,
} from "@/lib/bench";
import { highlight } from "@/lib/mdx";
import { pageMetadata } from "@/lib/metadata";
import { PageSchema } from "@/lib/structured-data";
import { repoBlob, repoTree, site } from "@site";

const r = perf.results;
const s = perf.settings;

export const metadata: Metadata = pageMetadata({
  title: "Performance",
  description: `${site.name}'s own measurements on a CRM-style API: ${fmtInt(r.deals_list.rps)} authenticated, paginated lists and ${fmtInt(
    r.deals_create.rps,
  )} validated inserts a second on ${s.app.count} CPUs, ${mb(perf.peakMemMb)} MB at peak, ready in ${perf.startupMs} ms. The workload, the method and how to reproduce it.`,
  path: "/performance/",
});

const sections = [
  { id: "results", label: "Results" },
  { id: "workload", label: "Workload" },
  { id: "method", label: "Method" },
  { id: "efficiency", label: "Efficiency" },
  { id: "next", label: "Next" },
  { id: "reproduce", label: "Reproduce" },
];

// ------------------------------------------------------------------ pieces

function SectionHead({
  id,
  eyebrow,
  title,
  children,
}: {
  id: string;
  eyebrow: string;
  title: string;
  children?: ReactNode;
}) {
  return (
    <div className="grid gap-x-12 gap-y-5 lg:grid-cols-12 lg:items-end">
      <div className="lg:col-span-7">
        <p className="eyebrow">{eyebrow}</p>
        <h2 id={`${id}-heading`} className="mt-5 max-w-3xl text-display-lg font-semibold text-balance text-fg">
          <a href={`#${id}`} className="heading-anchor">
            {title}
          </a>
        </h2>
      </div>
      {children ? (
        <div className="max-w-xl text-lg leading-relaxed text-pretty text-fg-2 lg:col-span-5 [&_code]:text-[0.875em]">
          {children}
        </div>
      ) : null}
    </div>
  );
}

function Section({ id, className, children }: { id: string; className?: string; children: ReactNode }) {
  return (
    <section
      id={id}
      aria-labelledby={`${id}-heading`}
      className={`cv-auto scroll-mt-12 border-t border-line first-of-type:border-t-0 ${className ?? ""}`}
    >
      <div className="container-site section-y">{children}</div>
    </section>
  );
}

/** A sub-section: its heading and text on the left, the chart on the right. */
function Split({ title, id, text, children }: { title: string; id: string; text: ReactNode; children: ReactNode }) {
  return (
    <div className="grid gap-x-12 gap-y-8 lg:grid-cols-12">
      <div className="lg:col-span-4">
        <h3 id={id} className="font-display text-display-sm font-semibold text-fg">
          {title}
        </h3>
        <div className="mt-3 space-y-3 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{text}</div>
      </div>
      <div className="min-w-0 lg:col-span-8">{children}</div>
    </div>
  );
}

function Code({ children }: { children: ReactNode }) {
  return <code className="inline-code">{children}</code>;
}

// ----------------------------------------------------------------- content

const endpoints: {
  method: "GET" | "POST";
  path: string;
  title: string;
  text: ReactNode;
  steps?: ReactNode[];
}[] = [
  {
    method: "GET",
    path: "/json",
    title: "Plain JSON",
    text: (
      <>
        Serializes a small struct per request. No database: this is routing and {site.name}&apos;s request pipeline on
        their own.
      </>
    ),
  },
  {
    method: "GET",
    path: "/db",
    title: "Single query",
    text: (
      <>
        Reads one random row of <Code>world</Code> by primary key, straight from the pool. A single read needs no
        transaction.
      </>
    ),
  },
  {
    method: "GET",
    path: "/queries?queries=20",
    title: "20 queries",
    text: <>Twenty separate random-row reads in a plain loop, one after another, with no batching.</>,
  },
  {
    method: "GET",
    path: "/api/deals?page=P&per_page=20",
    title: "Authenticated, paginated list",
    text: <>What a typical authenticated list endpoint does. The load generator cycles through pages 1 to 50.</>,
    steps: [
      <>
        Verify the bearer token: HS256 signature, <Code>exp</Code> and <Code>iss</Code>.
      </>,
      <>
        Load the user by the token&apos;s <Code>sub</Code> with the <Code>CurrentUser</Code> extractor.
      </>,
      <>
        Validate <Code>page</Code> ≥ 1 and <Code>per_page</Code> 1 to 100 with <Code>Valid&lt;Query&lt;T&gt;&gt;</Code>.
      </>,
      <>
        In the request&apos;s transaction: count the deals, load the page ordered by <Code>created_at DESC, id DESC</Code>,
        then the page&apos;s owners in one query.
      </>,
      <>
        Respond <Code>200</Code> with <Code>{"{ data, meta }"}</Code>, {site.name}&apos;s page shape.
      </>,
    ],
  },
  {
    method: "POST",
    path: "/api/deals",
    title: "Authenticated, validated insert",
    text: <>Every request creates a new deal, with a fresh title, amount and stage.</>,
    steps: [
      <>Verify the bearer token and load the user, as above.</>,
      <>
        Validate the body with <Code>Valid&lt;Json&lt;T&gt;&gt;</Code>: a title of 1 to 200 characters, an amount from 0
        to 10¹² cents, one of six stages.
      </>,
      <>
        <Code>INSERT … RETURNING</Code> in the request&apos;s unit of work, committed when the response succeeds.
      </>,
      <>
        Respond <Code>201</Code> with the deal and a <Code>Location</Code> header.
      </>,
    ],
  },
];

/** Navree's default request pipeline, outermost first (bench/frameworks/navree/README.md). */
const pipeline: { name: string; note: string }[] = [
  { name: "Request context", note: "Request id, client IP, tracing span" },
  { name: "Panic catcher", note: "A panic becomes a 500" },
  { name: "Timeout", note: "30 seconds" },
  { name: "Security headers", note: "HSTS, CSP, frame and referrer policy" },
  { name: "Compression", note: "When the client asks for it" },
  { name: "Error rendering", note: "One JSON error shape" },
  { name: "Maintenance mode", note: "Checked at most once a second" },
  { name: "Rate limit", note: "Per client IP" },
  { name: "Body limit", note: "2 MiB" },
  { name: "Authentication", note: "The JWT strategy" },
  { name: "Unit of work", note: "Commit below 400, roll back above" },
];

/** The per-request costs found by reading the code (bench/results/findings.html). */
const nextUp: { title: string; crate: string; affects: string; text: ReactNode }[] = [
  {
    title: "A ping before every query",
    crate: "navree-db",
    affects: "Every test that reads the database",
    text: (
      <>
        The pool keeps sqlx&apos;s <Code>test_before_acquire</Code> on, so each checkout pings PostgreSQL first. A single
        read costs two round trips instead of one.
      </>
    ),
  },
  {
    title: "An extra round trip per transaction",
    crate: "navree-db",
    affects: "Both deals endpoints",
    text: (
      <>
        Every unit-of-work transaction sends a separate <Code>SELECT set_config(…)</Code> for the actor and the request
        id before its first query.
      </>
    ),
  },
  {
    title: "A task for every request",
    crate: "navree-db",
    affects: "Every test",
    text: (
      <>
        The unit-of-work layer spawns a task per request so it completes even if the client disconnects, including on{" "}
        <Code>/json</Code>, where there is nothing to commit.
      </>
    ),
  },
  {
    title: "Process-wide locks on the hot path",
    crate: "navree-http",
    affects: "Every test",
    text: (
      <>
        Maintenance mode takes a <Code>Mutex</Code> and an <Code>RwLock</Code>, and the per-IP rate limiter one global{" "}
        <Code>Mutex&lt;HashMap&gt;</Code>, on every request.
      </>
    ),
  },
  {
    title: "Metrics work with no recorder",
    crate: "navree-http",
    affects: "Every test",
    text: (
      <>
        The request-context layer builds metric labels as fresh <Code>String</Code>s even when no metrics recorder is
        installed, along with a request scope and a UUID request id.
      </>
    ),
  },
  {
    title: "A wide user lookup",
    crate: "navree-auth",
    affects: "Both deals endpoints",
    text: (
      <>
        <Code>CurrentUser</Code> finds <Code>AuthService</Code> by scanning every provider, then loads every user
        column, including the password hash and two-factor secrets it doesn&apos;t need.
      </>
    ),
  },
  {
    title: "JSON bodies parsed twice",
    crate: "navree-validation",
    affects: "The insert",
    text: (
      <>
        A body is parsed into a <Code>serde_json::Value</Code> first, then into the DTO.
      </>
    ),
  },
  {
    title: "Change events nobody hears",
    crate: "navree-db",
    affects: "The insert",
    text: (
      <>
        <Code>Repo::insert</Code> serializes each new row for a change event even when nothing listens for change
        events.
      </>
    ),
  },
];

// -------------------------------------------------------------------- page

export default async function PerformancePage() {
  const checks = perf.conformance.passed ?? 0;
  const errors = totalErrors();
  const runs = measuredRuns();
  const tail = worstTailRatio();
  const repeatDelta = maxRepeatDelta();
  const appBusiest = appBusiestEverywhere();
  const navreeVersion = stackVersion("Framework", /Navree \d+\.\d+\.\d+/);
  const rustVersion = stackVersion("Runtime", /Rust \d+\.\d+\.\d+/);
  const repoName = site.repoUrl.replace(/\/$/, "").split("/").pop() ?? "navree";

  // ------------------------------------------------------- code, highlighted
  const strip = (html: string) => html.replace(/^<pre[^>]*>/, "").replace(/<\/pre>$/, "");
  const sample = {
    data: [
      {
        id: "5b49f24d-8106-1593-0363-db6dcf1cec2e",
        title: "Deal 1000",
        amount_cents: 4729000,
        stage: "won",
        owner: { id: "d6d77053-92bc-7af6-3332-8bea8c4c6904", name: "User 1" },
        created_at: "2026-01-01T16:40:00Z",
      },
      {
        id: "5f563e0d-3f5d-0e4b-e06a-aa1336b70037",
        title: "Deal 999",
        amount_cents: 4624271,
        stage: "negotiation",
        owner: { id: "ebb4bc01-7f24-c8dd-c7e6-2cd972adcc61", name: "User 100" },
        created_at: "2026-01-01T16:39:00Z",
      },
    ],
    meta: { current_page: 1, per_page: 20, total: 1000, last_page: 50 },
  };
  const controller = perf.code.find((c) => c.file.endsWith("controller.rs"));
  const dto = perf.code.find((c) => c.file.endsWith("dto.rs"));
  const panes: CodePane[] = [];
  if (controller) {
    panes.push({
      id: "controller",
      label: "controller.rs",
      caption: `${perf.sourceDir}/${controller.file}`,
      html: strip(await highlight(controller.source, "rust")),
    });
  }
  if (dto) {
    panes.push({
      id: "dto",
      label: "dto.rs",
      caption: `${perf.sourceDir}/${dto.file}`,
      html: strip(await highlight(dto.source, "rust")),
    });
  }
  panes.push({
    id: "response",
    label: "200 response",
    caption: "GET /api/deals?page=1&per_page=20",
    html: strip(await highlight(JSON.stringify(sample, null, 2), "json")),
    note: "The first two of the page's 20 deals. Every response is built per request from the database.",
  });

  const reproduce = [
    `git clone ${site.repoUrl}`,
    `cd ${repoName}`,
    "",
    `# Build, check conformance, then 5 s per test: a quick check`,
    `node bench/run.mjs --smoke --only navree`,
    "",
    `# The full run: ${s.warmupS} s warm-up, then ${s.runs} × ${s.durationS} s per test`,
    `node bench/run.mjs --only navree`,
  ].join("\n");
  const reproduceHtml = strip(await highlight(reproduce, "shellscript"));

  const figures = [
    { label: "Authenticated list", value: Math.round(r.deals_list.rps), unit: "req/s", decimals: 0 },
    { label: "Validated insert", value: Math.round(r.deals_create.rps), unit: "req/s", decimals: 0 },
    { label: "Plain JSON", value: Math.round(r.json.rps), unit: "req/s", decimals: 0 },
    { label: "Single query", value: Math.round(r.db.rps), unit: "req/s", decimals: 0 },
    { label: "Peak memory", value: mb(perf.peakMemMb), unit: "MB", decimals: Number.isInteger(mb(perf.peakMemMb)) ? 0 : 1 },
    { label: "Time to ready", value: perf.startupMs, unit: "ms", decimals: 0 },
  ];

  return (
    <>
      {/* ------------------------------------------------------------ hero */}
      <header className="relative overflow-hidden">
        <GridPattern
          width={48}
          height={48}
          squares={[
            [14, 2],
            [17, 4],
            [21, 1],
            [24, 5],
            [19, 7],
          ]}
          className="[mask-image:radial-gradient(ellipse_70%_80%_at_75%_30%,black,transparent_75%)] opacity-70"
        />
        <div className="container-site relative grid gap-x-12 gap-y-14 pt-14 pb-16 sm:pt-20 lg:grid-cols-12 lg:items-center lg:pt-24 lg:pb-20">
          <div className="lg:col-span-6 xl:col-span-6">
            <p className="eyebrow">Performance</p>
            <h1 className="mt-6 max-w-[15ch] text-display-xl font-semibold text-balance text-fg">
              Measured on the work a real API does.
            </h1>
            <p className="mt-6 max-w-xl text-lg leading-relaxed text-pretty text-fg-2">
              A CRM-style API with JWT auth, validation, pagination and inserts, on {s.app.count} CPUs against
              PostgreSQL. {site.name} serves {fmtInt(r.deals_list.rps)} authenticated, paginated lists and{" "}
              {fmtInt(r.deals_create.rps)} validated inserts a second, peaks at {mb(perf.peakMemMb)} MB and answers
              its first request {perf.startupMs} ms after it starts.
            </p>
            <p className="mt-4 max-w-xl text-[0.9375rem] leading-relaxed text-pretty text-fg-3">
              Every number on this page is read from one result file when the site is built. One command repeats the run.
            </p>
            <div className="mt-8 flex flex-wrap items-center gap-3">
              <Button asChild size="lg" className="pr-2 pl-6">
                <a href="#reproduce" className="group">
                  Reproduce it
                  <span
                    aria-hidden
                    className="inline-flex size-8 items-center justify-center rounded-full bg-accent-ink/10 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5"
                  >
                    <ArrowRightIcon className="size-4" />
                  </span>
                </a>
              </Button>
              <Button asChild size="lg" variant="outline" className="px-6">
                <a href="#method">Read the method</a>
              </Button>
            </div>
            <ul className="mt-10 flex flex-wrap gap-2" aria-label="About this run">
              {[`Measured ${measuredOn()}`, navreeVersion, rustVersion, "PostgreSQL 17"]
                .filter((x): x is string => Boolean(x))
                .map((x) => (
                  <li key={x}>
                    <Badge variant="mono" className="h-7 px-2.5 text-fg-2">
                      {x}
                    </Badge>
                  </li>
                ))}
            </ul>
          </div>

          {/* The readout: the result file's headline numbers. */}
          <div className="lg:col-span-6 xl:col-span-5 xl:col-start-8">
            <div className="bezel shadow-glow-lg">
              <div className="bezel-core">
                <div className="flex items-center justify-between gap-4 border-b border-line px-5 py-3">
                  <span className="truncate font-mono text-[0.75rem] text-fg-2">bench/results/navree.json</span>
                  {perf.conformance.ok ? (
                    <span className="flex shrink-0 items-center gap-2 font-mono text-[0.75rem] text-fg-1">
                      <span aria-hidden className="size-1.5 rounded-full bg-accent shadow-[0_0_8px_rgb(61_255_143/0.8)]" />
                      {checks}/{checks} checks
                    </span>
                  ) : null}
                </div>
                <dl className="grid grid-cols-2 gap-px bg-line">
                  {figures.map((f, i) => (
                    <div key={f.label} className="bg-ink-1 px-5 py-5 sm:px-6 sm:py-6">
                      <dt className="text-[0.8125rem] text-fg-2">{f.label}</dt>
                      <dd className="mt-2 flex flex-wrap items-baseline gap-x-1.5">
                        <TickerNumber
                          value={f.value}
                          decimals={f.decimals}
                          delay={0.15 + i * 0.07}
                          className="font-display text-[1.75rem] leading-none font-semibold tracking-[-0.035em] text-fg sm:text-[2.25rem]"
                        />
                        <span className="font-mono text-[0.75rem] text-fg-3">{f.unit}</span>
                      </dd>
                    </div>
                  ))}
                </dl>
                <p className="border-t border-line px-5 py-3 font-mono text-[0.75rem] leading-relaxed text-fg-3">
                  Median of {s.runs} × {s.durationS} s runs · {s.connections} connections · {s.app.count} CPUs ·{" "}
                  {errors === 0 ? "0 errors" : `${fmtInt(errors)} errors`}
                </p>
              </div>
            </div>
          </div>

        </div>
      </header>

      <SectionNav
        sections={sections}
        aside={
          <a
            href={perf.rawHref}
            download
            className="inline-flex items-center gap-2 rounded-md font-mono text-[0.75rem] text-fg-3 transition-colors hover:text-fg"
          >
            <DownloadIcon className="size-3.5" />
            navree.json
          </a>
        }
      />

      {/* --------------------------------------------------------- results */}
      <Section id="results">
        <SectionHead id="results" eyebrow="Results" title="Five tests, from plain JSON to authenticated writes.">
          <p>
            Each figure is the median of {s.runs} runs of {s.durationS} seconds at {s.connections} open connections, after
            a {s.warmupS}-second warm-up.{" "}
            {errors === 0 ? `None of the ${runs} runs returned an error.` : `${fmtInt(errors)} requests failed across ${runs} runs.`}
          </p>
        </SectionHead>

        <div className="mt-12 lg:mt-16">
          <ThroughputBoard />
        </div>

        <div className="mt-20 space-y-20 lg:mt-28 lg:space-y-28">
          <Split
            id="latency"
            title="Latency"
            text={
              <>
                <p>
                  The time each request took, measured by the load generator with every connection busy. The median
                  sits between {r.json.latencyMs.p50.toFixed(1)} ms for plain JSON and {fmtInt(r.queries.latencyMs.p50)} ms
                  for twenty queries in a row.
                </p>
                <p>p99 stays within {tail.toFixed(1)}× of the median in every test: no long tail.</p>
              </>
            }
          >
            <LatencyChart />
          </Split>

          <Split
            id="cpu"
            title="Where the CPU goes"
            text={
              <>
                <p>
                  On plain JSON the app used {cpus(r.json.cpuPct)} of its {s.app.count} CPUs while PostgreSQL did nothing,
                  so that test is {site.name}&apos;s request pipeline alone.
                </p>
                {appBusiest ? (
                  <p>
                    The app&apos;s CPUs are the busiest of the three in every test. {site.name}&apos;s own per-request work
                    sets the pace, which is what the{" "}
                    <a href="#next" className="link">
                      next round of work
                    </a>{" "}
                    goes after.
                  </p>
                ) : null}
              </>
            }
          >
            <CpuChart />
          </Split>

          <div>
            <h3 id="memory" className="font-display text-display-sm font-semibold text-fg">
              Memory and startup
            </h3>
            <p className="mt-3 max-w-2xl text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
              One process, measured from outside the container: what the app holds at rest, at its busiest, and how soon
              it answers.
            </p>
            <div className="mt-8">
              <MemoryAndStartup />
            </div>
          </div>
        </div>
      </Section>

      {/* -------------------------------------------------------- workload */}
      <Section id="workload">
        <SectionHead id="workload" eyebrow="Workload" title="A small CRM API, and nothing tuned for it.">
          <p>
            The app is an ordinary {site.name} application: modules and controllers, the JWT strategy and{" "}
            <Code>CurrentUser</Code>, validated extractors, ORM models, the unit of work and {site.name}&apos;s
            pagination. It reads a seeded PostgreSQL database of 10,000 <Code>world</Code> rows, 100 users and 1,000 deals.
          </p>
        </SectionHead>

        <div className="mt-12 grid gap-x-12 gap-y-12 lg:mt-16 lg:grid-cols-12">
          <ol className="lg:col-span-6">
            {endpoints.map((e) => (
              <li key={e.path} className="border-t border-line py-6 first:border-t-0 first:pt-0">
                <p className="flex flex-wrap items-center gap-2.5">
                  <span
                    className={`rounded-md border px-1.5 py-0.5 font-mono text-[0.6875rem] font-medium ${
                      e.method === "POST" ? "border-accent/30 bg-accent/10 text-accent" : "border-line-2 bg-ink-2 text-fg-1"
                    }`}
                  >
                    {e.method}
                  </span>
                  <code className="font-mono text-[0.875rem] break-all text-fg">{e.path}</code>
                </p>
                <h3 className="mt-3 font-medium text-fg">{e.title}</h3>
                <p className="mt-1.5 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{e.text}</p>
                {e.steps ? (
                  <ol className="mt-4 space-y-2.5">
                    {e.steps.map((step, i) => (
                      <li key={i} className="grid grid-cols-[1.75rem_minmax(0,1fr)] text-[0.9375rem] leading-relaxed text-fg-2">
                        <span className="pt-px font-mono text-[0.75rem] leading-[1.7rem] text-fg-3">{String(i + 1).padStart(2, "0")}</span>
                        <span className="text-pretty">{step}</span>
                      </li>
                    ))}
                  </ol>
                ) : null}
              </li>
            ))}
          </ol>

          <div className="min-w-0 lg:col-span-6">
            <div className="lg:sticky lg:top-[calc(var(--header-h)+5rem)]">
              <CodePanel panes={panes} label="The deals endpoints" />
              <p className="mt-4 text-sm leading-relaxed text-fg-3">
                The code being measured, from{" "}
                <a href={repoTree(perf.sourceDir)} className="link">
                  {perf.sourceDir}
                </a>
                . Its README lists every version and setting.
              </p>
            </div>
          </div>
        </div>

        {/* The pipeline every request goes through. */}
        <div className="mt-20 lg:mt-28">
          <Split
            id="pipeline"
            title="The full default pipeline, on every request"
            text={
              <>
                <p>
                  Nothing is switched off for speed. Every request passes through {site.name}&apos;s default layers,
                  outermost first, then the router, the route&apos;s access check and the handler.
                </p>
                <p>
                  The rate limit stays in the pipeline with a very high ceiling, because the load generator is one client
                  IP.
                </p>
              </>
            }
          >
            <ol className="grid grid-cols-2 gap-px overflow-hidden rounded-2xl border border-line bg-line xl:grid-cols-3">
              {pipeline.map((layer, i) => (
                <li key={layer.name} className="flex items-start gap-3 bg-ink-0 px-4 py-4 sm:gap-4 sm:px-5">
                  <span className="pt-0.5 font-mono text-[0.75rem] text-fg-3 tabular-nums">{String(i + 1).padStart(2, "0")}</span>
                  <span className="min-w-0">
                    <span className="block text-[0.9375rem] font-medium text-fg-1">{layer.name}</span>
                    <span className="mt-0.5 block text-[0.8125rem] text-fg-3">{layer.note}</span>
                  </span>
                </li>
              ))}
              <li className="flex items-start gap-3 bg-accent/[0.06] px-4 py-4 sm:gap-4 sm:px-5">
                <span className="pt-0.5 font-mono text-[0.75rem] text-accent tabular-nums">12</span>
                <span className="min-w-0">
                  <span className="block text-[0.9375rem] font-medium text-fg">Your handler</span>
                  <span className="mt-0.5 block text-[0.8125rem] text-fg-2">After the router and the access check</span>
                </span>
              </li>
            </ol>
          </Split>
        </div>
      </Section>

      {/* ---------------------------------------------------------- method */}
      <Section id="method">
        <SectionHead id="method" eyebrow="Method" title="One machine, three containers, fixed CPUs.">
          <p>
            The app runs alone, pinned to {s.app.count} CPUs. PostgreSQL and the load generator have CPUs of their own, so
            nothing competes with the app for time.
          </p>
        </SectionHead>

        <div className="mt-12 lg:mt-16">
          <MachineDiagram />
        </div>

        <div className="mt-20 grid gap-x-12 gap-y-14 lg:mt-28 lg:grid-cols-12">
          <div className="lg:col-span-7">
            <h3 className="font-display text-display-sm font-semibold text-fg">How a run goes</h3>
            <ol className="mt-8 space-y-0">
              {[
                {
                  title: "Build",
                  text: (
                    <>
                      A release build (<Code>opt-level = 3</Code>, thin LTO, one codegen unit) in a slim Debian image.
                    </>
                  ),
                },
                {
                  title: "Fresh data",
                  text: <>The database is recreated from the seeded template before the app starts, so every run sees the same rows.</>,
                },
                {
                  title: "Start",
                  text: (
                    <>
                      The clock runs from the container starting to the first <Code>200</Code> from <Code>GET /json</Code>.
                      Idle memory is read once it answers.
                    </>
                  ),
                },
                {
                  title: "Conformance",
                  text: (
                    <>
                      {checks} checks against the API contract: response shapes, status codes, every way a token can be
                      wrong, validation failures and pagination edges. One failure and nothing is measured.{" "}
                      {site.name} passed {perf.conformance.passed} and failed {perf.conformance.failed}.
                    </>
                  ),
                },
                { title: "Warm up", text: <>{s.warmupS} seconds of load per test, not measured.</> },
                {
                  title: "Measure",
                  text: (
                    <>
                      {s.runs} runs of {s.durationS} seconds per test with wrk, {s.threads} threads and {s.connections}{" "}
                      connections. The median run is reported; memory and CPU are sampled with{" "}
                      <Code>docker stats</Code> throughout.
                    </>
                  ),
                },
              ].map((step, i, all) => (
                <li key={step.title} className="relative grid grid-cols-[2.5rem_minmax(0,1fr)] gap-x-4 pb-8 last:pb-0">
                  {i < all.length - 1 ? (
                    <span aria-hidden className="absolute top-9 bottom-1 left-[1.25rem] w-px -translate-x-1/2 bg-line" />
                  ) : null}
                  <span className="flex size-10 items-center justify-center rounded-full border border-line-2 bg-ink-1 font-mono text-[0.75rem] text-fg-1 tabular-nums">
                    {String(i + 1).padStart(2, "0")}
                  </span>
                  <div className="pt-2">
                    <p className="font-medium text-fg">{step.title}</p>
                    <p className="mt-1.5 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{step.text}</p>
                  </div>
                </li>
              ))}
            </ol>
          </div>

          <div className="space-y-3 lg:col-span-5">
            {[
              {
                title: "wrk on the monotonic clock",
                text: (
                  <>
                    Stock wrk times requests with the wall clock. The VM under Docker Desktop steps its wall clock now and
                    then, which showed up as phantom timeouts. The harness builds wrk 4.2.0 with every clock read switched
                    to <Code>CLOCK_MONOTONIC</Code>.
                  </>
                ),
              },
              {
                title: "Production settings",
                text: (
                  <>
                    Release build, log level <Code>warn</Code> (no per-request log line), one pool of 64 PostgreSQL
                    connections, and {site.name}&apos;s default pipeline on, as above.
                  </>
                ),
              },
              {
                title: "Laptop numbers",
                text: (
                  <>
                    One {perf.machine.cpu} laptop with {s.ncpu} CPUs, {perf.machine.host}, {perf.machine.network}. A
                    server will give different absolute figures; the method and the shape of the results carry over.
                  </>
                ),
              },
            ].map((note) => (
              <div key={note.title} className="surface p-6">
                <p className="font-medium text-fg">{note.title}</p>
                <p className="mt-2 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{note.text}</p>
              </div>
            ))}
          </div>
        </div>

        {perf.repeat ? (
          <div className="mt-20 lg:mt-28">
            <Split
              id="steady"
              title="Steady from run to run"
              text={
                <>
                  <p>
                    The three runs of each test land close together. {site.name} was then measured again{" "}
                    {perf.repeat.minutesLater} minutes later, after the machine had been under load the whole time
                    {repeatDelta !== null ? `: every test within ${repeatDelta.toFixed(1)}% of the first run` : ""}.
                  </p>
                  <p>The machine held steady for the whole session: no thermal slowdown.</p>
                </>
              }
            >
              <RepeatChart />
            </Split>
          </div>
        ) : null}

        <div className="mt-20 lg:mt-28">
          <Split
            id="build"
            title={`${site.name}'s build`}
            text={
              <p>
                The versions and settings of the measured app, read from its{" "}
                <a href={repoBlob(`${perf.sourceDir}/README.md`)} className="link">
                  README
                </a>
                .
              </p>
            }
          >
            <dl className="divide-y divide-line overflow-hidden rounded-2xl border border-line">
              {perf.stack.map((row) => (
                <div key={row.label} className="grid gap-x-6 gap-y-1.5 px-5 py-4 sm:grid-cols-[8rem_minmax(0,1fr)] sm:px-6">
                  <dt className="font-mono text-[0.75rem] tracking-[0.06em] text-fg-3 uppercase sm:pt-0.5">{row.label}</dt>
                  <dd
                    className="text-[0.9375rem] leading-relaxed break-words text-fg-1 [&_code]:rounded [&_code]:bg-ink-2 [&_code]:px-1 [&_code]:py-px [&_code]:font-mono [&_code]:text-[0.8125rem] [&_code]:text-fg"
                    // Shiki's output for a string that lives in this repository, highlighted at build time
                    // (Shiki escapes the code it highlights).
                    // nosemgrep: navree-raw-html
                    dangerouslySetInnerHTML={{ __html: row.html }}
                  />
                </div>
              ))}
            </dl>
          </Split>
        </div>
      </Section>

      {/* ------------------------------------------------------ efficiency */}
      <Section id="efficiency">
        <SectionHead id="efficiency" eyebrow="Efficiency" title="Why it stays lean.">
          <p>None of this is a benchmark setting. It is how every {site.name} app runs.</p>
        </SectionHead>

        <ul className="mt-12 grid gap-px overflow-hidden rounded-2xl border border-line bg-line sm:grid-cols-2 lg:mt-16 lg:grid-cols-3">
          {[
            {
              key: "binary",
              title: "One binary",
              text: "Your app, its modules and the framework compile to one native executable. Deploy the file; there is no runtime to install beside it.",
              proof: `Ready in ${perf.startupMs} ms · idle at ${mb(perf.idleMemMb)} MB`,
            },
            {
              key: "tokio",
              title: "Tokio underneath",
              text: "One multi-threaded runtime with a worker per CPU. Every connection and every request is a lightweight task, so open connections cost no threads.",
              proof: `${s.app.count} workers · ${s.connections} connections`,
            },
            {
              key: "hyper",
              title: "Hyper and Tower",
              text: `Hyper speaks HTTP, ${site.name} routes, and its request pipeline is a stack of Tower layers around your handlers.`,
              proof: `${fmtInt(r.json.rps)} JSON responses a second`,
            },
            {
              key: "pool",
              title: "Pooled connections",
              text: "One pool of PostgreSQL connections, kept open and shared by every worker. The ORM builds the queries and sqlx runs them.",
              proof: `${fmtInt(r.db.rps)} single queries a second`,
            },
            {
              key: "gc",
              title: "No garbage collector",
              text: "Rust frees memory the moment a value goes out of scope. There are no collection pauses to tune, and memory tracks the work in flight.",
              proof: `${mb(perf.peakMemMb)} MB at peak · p99 within ${tail.toFixed(1)}× of p50`,
            },
            {
              key: "async",
              title: "Async all the way down",
              text: "The HTTP stack, the pool and every query are async, so a worker never sits blocked while PostgreSQL answers. It serves another request.",
              proof: `${fmtInt(r.queries.rps)} requests a second at 20 queries each`,
            },
          ].map((item, i) => (
            <li key={item.key} className="flex flex-col bg-ink-0 p-6 sm:p-8">
              <span className="font-mono text-[0.75rem] text-fg-3 tabular-nums">{String(i + 1).padStart(2, "0")}</span>
              <h3 className="mt-6 font-display text-display-sm font-semibold text-fg">{item.title}</h3>
              <p className="mt-3 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{item.text}</p>
              <p className="mt-auto flex items-start gap-2 pt-8 font-mono text-[0.75rem] leading-snug text-fg-1">
                <span aria-hidden className="mt-[0.3rem] size-1.5 shrink-0 rounded-full bg-accent" />
                {item.proof}
              </p>
            </li>
          ))}
        </ul>
      </Section>

      {/* ------------------------------------------------------------ next */}
      <Section id="next">
        <SectionHead id="next" eyebrow="Next" title="What we're making faster next.">
          <p>
            {site.name}&apos;s own per-request work is where the time goes: on a single query the app used{" "}
            {cpus(r.db.cpuPct)} of its {s.app.count} CPUs while PostgreSQL used {cpus(r.db.pgCpuPct)} of {s.pg.count}.
            Reading the code turned up eight costs, and removing each one takes work off every request it touches.
          </p>
        </SectionHead>

        <ol className="mt-12 grid gap-x-12 lg:mt-16 lg:grid-cols-2">
          {nextUp.map((item, i) => (
            <li
              key={item.title}
              className={`grid grid-cols-[2.5rem_minmax(0,1fr)] gap-x-4 border-t border-line py-7 ${i < 2 ? "lg:border-t-0 lg:pt-0" : ""} ${i === 0 ? "border-t-0 pt-0" : ""}`}
            >
              <span className="pt-1 font-mono text-[0.8125rem] text-fg-3 tabular-nums">{String(i + 1).padStart(2, "0")}</span>
              <div className="min-w-0">
                <h3 className="text-[1.0625rem] font-medium text-fg">{item.title}</h3>
                <p className="mt-2 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{item.text}</p>
                <p className="mt-4 flex flex-wrap items-center gap-2">
                  <Badge variant="mono">{item.crate}</Badge>
                  <span className="text-[0.8125rem] text-fg-3">{item.affects}</span>
                </p>
              </div>
            </li>
          ))}
        </ol>
      </Section>

      {/* ------------------------------------------------------- reproduce */}
      <Section id="reproduce">
        <SectionHead id="reproduce" eyebrow="Reproduce" title="Run it yourself.">
          <p>
            You need Docker and Node.js. The script starts PostgreSQL, builds wrk and the app, runs the conformance test,
            measures every test and writes <Code>bench/results/navree.json</Code>.
          </p>
        </SectionHead>

        <div className="mt-12 grid gap-x-12 gap-y-14 lg:mt-16 lg:grid-cols-12">
          <div className="min-w-0 lg:col-span-7">
            <div className="bezel [&_.code-block]:my-0">
              <CodeBlock data-title="Terminal" data-language="shellscript" className="shiki navree-night scroll-thin">
                <span
                  // Shiki's output for a string that lives in this repository, highlighted at build time
                  // (Shiki escapes the code it highlights).
                  // nosemgrep: navree-raw-html
                  dangerouslySetInnerHTML={{ __html: reproduceHtml }}
                />
              </CodeBlock>
            </div>

            <h3 className="mt-12 font-medium text-fg">Fit it to your machine</h3>
            <p className="mt-2 text-[0.9375rem] leading-relaxed text-fg-2">
              The defaults expect {s.ncpu} CPUs. Every setting is a flag:
            </p>
            <dl className="mt-5 divide-y divide-line overflow-hidden rounded-2xl border border-line text-[0.9375rem]">
              {[
                [`--app-cpus ${s.app.range}`, "The app's CPUs"],
                [`--pg-cpus ${s.pg.range}`, "PostgreSQL's CPUs"],
                [`--load-cpus ${s.load.range}`, "The load generator's CPUs"],
                [`--runs ${s.runs} --duration ${s.durationS}`, "Measured runs per test, and seconds each"],
                [`--warmup ${s.warmupS}`, "Seconds of warm-up per test"],
                [`--connections ${s.connections} --threads ${s.threads}`, "wrk's connections and threads"],
                ["--skip-build", "Reuse the images from the last run"],
              ].map(([flag, what]) => (
                <div key={flag} className="grid gap-x-6 gap-y-1 px-5 py-3.5 sm:grid-cols-[minmax(0,15rem)_minmax(0,1fr)] sm:items-baseline sm:px-6">
                  <dt className="font-mono text-[0.8125rem] break-words text-fg">{flag}</dt>
                  <dd className="text-fg-2">{what}</dd>
                </div>
              ))}
            </dl>
          </div>

          <div className="space-y-12 lg:col-span-5">
            <div>
              <h3 className="font-medium text-fg">The raw results</h3>
              <p className="mt-2 text-[0.9375rem] leading-relaxed text-fg-2">
                The files this page is built from, with every run, percentile and sample.
              </p>
              <ul className="mt-5 divide-y divide-line overflow-hidden rounded-2xl border border-line">
                {perf.downloads.map((d) => (
                  <li key={d.file}>
                    <a
                      href={d.href}
                      download
                      className="group flex items-center justify-between gap-4 px-5 py-4 transition-colors hover:bg-ink-2"
                    >
                      <span className="min-w-0">
                        <span className="block font-mono text-[0.8125rem] text-fg">{d.file}</span>
                        <span className="mt-1 block text-[0.8125rem] text-fg-3">{d.what}</span>
                      </span>
                      <span className="flex shrink-0 items-center gap-2 font-mono text-[0.75rem] text-fg-3 group-hover:text-fg-1">
                        {fmtInt(d.bytes / 1024)} KB <DownloadIcon className="size-3.5" />
                      </span>
                    </a>
                  </li>
                ))}
              </ul>
            </div>

            <div>
              <h3 className="font-medium text-fg">The source</h3>
              <ul className="mt-5 divide-y divide-line overflow-hidden rounded-2xl border border-line">
                {[
                  { label: `${site.name}'s benchmark app`, path: perf.sourceDir, tree: true },
                  { label: "The API contract", path: "bench/SPEC.md", tree: false },
                  { label: "The conformance test", path: "bench/tools/conformance.mjs", tree: false },
                  { label: "The load generator and its scripts", path: "bench/wrk", tree: true },
                  { label: "The seed data", path: "bench/db/init.sql", tree: false },
                  { label: "The harness", path: "bench/run.mjs", tree: false },
                ].map((link) => (
                  <li key={link.path}>
                    <a
                      href={link.tree ? repoTree(link.path) : repoBlob(link.path)}
                      className="group flex items-center justify-between gap-4 px-5 py-3.5 transition-colors hover:bg-ink-2"
                    >
                      <span className="min-w-0">
                        <span className="block text-[0.9375rem] text-fg-1 group-hover:text-fg">{link.label}</span>
                        <span className="mt-0.5 block font-mono text-[0.75rem] break-all text-fg-3">{link.path}</span>
                      </span>
                      <ExternalIcon className="size-3.5 shrink-0 text-fg-3 group-hover:text-fg-1" />
                    </a>
                  </li>
                ))}
              </ul>
            </div>
          </div>
        </div>
      </Section>

      {/* ----------------------------------------------------------- close */}
      <section aria-labelledby="perf-cta" className="cv-auto relative overflow-hidden border-t border-line">
        <GridPattern
          width={48}
          height={48}
          className="[mask-image:radial-gradient(ellipse_60%_90%_at_50%_100%,black,transparent_70%)] opacity-60"
        />
        <div className="container-site relative flex flex-col items-start gap-8 py-20 sm:py-24 lg:flex-row lg:items-end lg:justify-between">
          <div>
            <h2 id="perf-cta" className="max-w-2xl text-display-md font-semibold text-balance text-fg">
              The same pipeline runs in the app you build.
            </h2>
            <p className="mt-4 max-w-xl text-lg leading-relaxed text-pretty text-fg-2">
              Every layer measured here is on by default in a new {site.name} project. Start one, put it under load and
              read your own numbers.
            </p>
          </div>
          <div className="flex flex-wrap gap-3">
            <Button asChild size="lg" className="pr-2 pl-6">
              <Link href={site.getStarted} className="group">
                Get started
                <span
                  aria-hidden
                  className="inline-flex size-8 items-center justify-center rounded-full bg-accent-ink/10 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5"
                >
                  <ArrowRightIcon className="size-4" />
                </span>
              </Link>
            </Button>
            <Button asChild size="lg" variant="outline" className="px-6">
              <Link href="/docs/">Read the docs</Link>
            </Button>
          </div>
        </div>
      </section>
      <PageSchema path="/performance/" metadata={metadata} />
    </>
  );
}
