import type { Metadata } from "next";
import Link from "next/link";
import type { ReactNode } from "react";

import measured from "@content/showcase/store.json";
import { PrimaryCta, SecondaryCta, SectionHeading } from "@/components/home/parts";
import { CodePanel, CommandLine } from "@/components/showcase/code-panel";
import { example } from "@/components/ui-docs/data";
import { pageMetadata } from "@/lib/metadata";
import { PageSchema } from "@/lib/structured-data";
import { cn } from "@/lib/utils";
import { repoTree } from "@site";

import { MoreApps } from "@/components/showcase/more-apps";
import { Preview } from "@/components/showcase/preview";
import { showcases } from "../../ui/showcases";

export const metadata: Metadata = pageMetadata({
  title: "A store's back office, built on Navree",
  description:
    "An example app on Navree UI: 200,000 orders on SQLite paged by cursor, an order drawer over its page, refunds checked on the server, a live feed. Measured.",
  path: "/showcase/store/",
});

type Timing = { ttfb_ms: number; fcp_ms: number; lcp_ms: number; html_bytes: number; html_decoded_bytes: number; total_bytes: number; requests: number };
type Measured = {
  measured_on: string;
  machine: string;
  method: string;
  rows: { orders: number; lines: number; events: number; customers: number; products: number; bytes: number; seed_seconds: number };
  pages: { name: string; path: string; desktop: Timing; phone: Timing }[];
  drawer: { open_ms: number; parts_ms: number; bytes: number };
  plans: { name: string; statements: { path: string; sql: string; ms: number; plan: string[] }[] }[];
};
const data = measured as Measured;
const store = showcases.find((s) => s.id === "store")!;
const SOURCE = "apps/store-demo";

const number = (n: number) => n.toLocaleString("en-US");
const kb = (bytes: number) => `${(bytes / 1024).toFixed(1)} KB`;
const seconds = (ms: number) => `${(ms / 1000).toFixed(2)} s`;
const page = (name: string) => data.pages.find((p) => p.name === name)!;

/** A section of this page: a hairline on top, the container and the rhythm. */
function Section({ id, children, className }: { id: string; children: ReactNode; className?: string }) {
  return (
    <section id={id} aria-labelledby={`${id}-heading`} className={cn("cv-auto scroll-mt-(--header-h) border-t border-line", className)}>
      <div className="container-site section-y">{children}</div>
    </section>
  );
}

/** The first top-level item of an example's code: what the app wrote, read by the UI docs. */
function firstItem(id: string): string {
  return example(id).code.split(/\n\n(?=(?:\/\/\/|#\[|pub |fn ))/)[0];
}

/** What a plan says, in words: which index each step reads by, and how. */
function readsBy(plan: string[]): string {
  const words = plan
    .filter((line) => !/^(CO-ROUTINE|SCAN counted)/.test(line))
    .map((line) => {
      const key = line.match(/^SEARCH (\w+) USING INTEGER PRIMARY KEY/);
      if (key) return `finds one row of ${key[1]} by its key`;
      const counted = line.match(/^SEARCH (\w+) USING COVERING INDEX (\w+) \((.+)\)/);
      if (counted) return `counts in the index ${counted[2]} alone, where ${counted[3]}`;
      const found = line.match(/^SEARCH (\w+) USING INDEX (\w+) \((.+)\)/);
      if (found) return `finds the rows of ${found[1]} through the index ${found[2]}, where ${found[3]}`;
      const capped = line.match(/^SCAN (\w+) USING COVERING INDEX (\w+)/);
      if (capped) return `counts through the index ${capped[2]} alone, stopping at the cap`;
      const walked = line.match(/^SCAN (\w+) USING INDEX (\w+)/);
      if (walked) return `walks ${walked[1]} in the order of the index ${walked[2]}, stopping at the page's end`;
      return line.replace(/^SCAN (\w+)$/, "reads every row of $1");
    });
  return words.join("; ");
}

function Hero() {
  const orders = page("Orders");
  const facts = [
    { value: number(data.rows.orders), label: "orders on SQLite", text: `${number(data.rows.events)} events, ${number(data.rows.products)} products` },
    { value: seconds(orders.phone.lcp_ms), label: "to the first screen of orders", text: "On a slow phone: 4x CPU, Slow 4G" },
    { value: kb(orders.desktop.html_bytes), label: "of HTML for 25 orders", text: "Compressed, as it goes over the wire" },
    { value: `${data.drawer.open_ms} ms`, label: "from a click to the drawer", text: `Its history in ${data.drawer.parts_ms} ms` },
  ];
  return (
    <section aria-labelledby="store-heading" className="relative isolate overflow-hidden">
      <div
        aria-hidden
        className="pointer-events-none absolute -top-40 right-[-10rem] -z-10 h-[40rem] w-[56rem] bg-[radial-gradient(ellipse_at_center,rgb(61_255_143/0.08),transparent_65%)]"
      />
      <div className="container-site pt-14 pb-12 sm:pt-20 lg:pt-28 lg:pb-16">
        <div className="max-w-3xl">
          <p className="eyebrow">
            <Link href="/showcase/" className="hover:text-fg">
              Showcase
            </Link>
          </p>
          <h1 id="store-heading" className="mt-5 text-display-xl font-semibold text-balance text-fg">
            A store&apos;s back office, built on Navree.
          </h1>
          <p className="mt-6 max-w-2xl text-lg leading-relaxed text-pretty text-fg-2">
            Charkha Supply Co. is an invented shop in Punjab that sells kitchen, pantry, clothing and festive goods across
            India, with {number(data.rows.orders)} orders on SQLite. Every screen on this page is the app&apos;s own Rust,
            rendered by navree-ui: orders paged by cursor on the server, an order in a drawer over its page, refunds and
            cancels the server checks again, and a feed that streams only what is new.
          </p>
          <div className="mt-8 flex flex-wrap items-center gap-3">
            <PrimaryCta href="#run">Run it locally</PrimaryCta>
            <SecondaryCta href={repoTree(SOURCE)}>Source on GitHub</SecondaryCta>
          </div>
        </div>
        <dl className="mt-12 grid grid-cols-1 gap-px overflow-hidden rounded-2xl border border-line bg-line sm:grid-cols-2 lg:mt-16 lg:grid-cols-4">
          {facts.map((f) => (
            <div key={f.label} className="bg-ink-1 p-5 sm:p-6">
              <dt className="sr-only">{f.label}</dt>
              <dd>
                <p className="font-display text-display-md font-semibold text-fg numeric">{f.value}</p>
                <p className="mt-1 text-[0.9375rem] font-medium text-fg-1">{f.label}</p>
                <p className="mt-1 text-sm text-fg-3">{f.text}</p>
              </dd>
            </div>
          ))}
        </dl>
      </div>
    </section>
  );
}

function Screens() {
  return (
    <Section id="screens">
      <SectionHeading
        split
        id="screens-heading"
        eyebrow="The screens"
        title="Four screens, each the app's own view."
        lead={
          <p>
            The UI docs call the app&apos;s view functions with its seed and render them with navree-ui, so what you see here is
            what the app draws. The drawer can be scrolled; links and forms go nowhere outside the app.
          </p>
        }
      />
      <div className="mt-12 space-y-14 lg:mt-16 lg:space-y-20">
        {store.views.map((view) => (
          <div key={view.example}>
            <h3 className="mb-4 font-display text-display-sm font-semibold text-fg">{view.label}</h3>
            <Preview id={view.example} title={view.title} caption={view.caption} interactive={view.interactive} defer />
          </div>
        ))}
      </div>
    </Section>
  );
}

const components: { name: string; href: string; use: string }[] = [
  { name: "DataTable", href: "/docs/ui/data-table/", use: "The orders in cursor mode, and the inventory: filters, search, sort and pages on the server, refreshed by the stream" },
  { name: "Sheet", href: "/docs/components/sheet/", use: "The order drawer, SheetWidth::Xl, 60% of the screen over its page" },
  { name: "Dialog and AlertDialog", href: "/docs/components/alert-dialog/", use: "Refund, cancel and resend, each asked first, with a form that posts to the server" },
  { name: "FactCard", href: "/docs/components/fact-card/", use: "The customer, the payment and both ends of the route, with flags" },
  { name: "Timeline", href: "/docs/components/timeline/", use: "An order's steps from placed to delivered, the ones to come drawn as rings" },
  { name: "ActivityFeed", href: "/docs/components/activity-feed/", use: "The order activity, live, fetching only what came after its newest row" },
  { name: "Region", href: "/docs/ui/regions/", use: "The drawer's history and the customer's other orders, loaded after it opens; the figures, live" },
  { name: "Badge, Stat, Table, Toast, Flag", href: "/docs/components/badge/", use: "Statuses, today's figures, an order's lines, what each action did, and the countries" },
];

const modules: { name: string; use: string }[] = [
  { name: "HttpModule", use: "The routes and the compressed answers" },
  { name: "RealtimeModule", use: "The live hub: one server-sent event stream a page, nudged after each change commits" },
  { name: "DatabaseModule and the ORM", use: "Five models on SQLite, a transaction for the seed, and conditional updates so two refunds at once can't give back more than was paid" },
  { name: "UiModule", use: "The shell, the stylesheet and the scripts each page asks for" },
  { name: "Forms", use: "Submit and Validate read the refund and the restock, with the same checks with JavaScript off" },
  { name: "The kernel's lifecycle", use: "Seeds the shop when the app starts, and runs the simulator until it stops" },
];

function Built() {
  const excerpts = [
    { file: `${SOURCE}/src/views/orders.rs`, source: firstItem("build-anything/store-orders") },
    { file: `${SOURCE}/src/views/order.rs`, source: firstItem("build-anything/store-drawer") },
    { file: `${SOURCE}/src/views/dashboard.rs`, source: firstItem("build-anything/store-dashboard") },
  ];
  return (
    <Section id="built">
      <SectionHeading
        split
        id="built-heading"
        eyebrow="How it is built"
        title="Toolkit components on a framework app."
        lead={<p>One Rust crate: its views are plain functions of its data, and the server runs every rule the screens show.</p>}
      />
      <div className="mt-12 grid gap-12 lg:mt-16 lg:grid-cols-2">
        <div>
          <h3 className="font-display text-display-sm font-semibold text-fg">From Navree UI</h3>
          <ul className="mt-4">
            {components.map((c) => (
              <li key={c.name} className="border-t border-line py-3 text-[0.9375rem] leading-relaxed text-pretty">
                <Link href={c.href} className="font-medium text-fg-1 underline decoration-line-2 underline-offset-4 hover:text-accent hover:decoration-accent">
                  {c.name}
                </Link>
                <span className="text-fg-2">: {c.use}</span>
              </li>
            ))}
          </ul>
        </div>
        <div>
          <h3 className="font-display text-display-sm font-semibold text-fg">From the framework</h3>
          <ul className="mt-4">
            {modules.map((m) => (
              <li key={m.name} className="border-t border-line py-3 text-[0.9375rem] leading-relaxed text-pretty">
                <span className="font-medium text-fg-1">{m.name}</span>
                <span className="text-fg-2">: {m.use}</span>
              </li>
            ))}
          </ul>
        </div>
      </div>
      <div className="mt-12 grid gap-3 xl:grid-cols-3">
        {excerpts.map((e) => (
          <CodePanel key={e.file} file={e.file} lang="rust" source={e.source} className="[&_pre]:max-h-[28rem] [&_pre]:overflow-y-auto" />
        ))}
      </div>
    </Section>
  );
}

function Scale() {
  return (
    <Section id="scale">
      <SectionHeading
        split
        id="scale-heading"
        eyebrow="Proven at size"
        title={`${number(data.rows.orders)} orders, measured.`}
        lead={
          <p>
            Lists load a page at a time, never everything at once, and the drawer loads its slow parts after it opens. Every
            number here comes from <code className="inline-code">e2e/ui/measure-store.mjs</code>, run on {data.measured_on}.
          </p>
        }
      />
      <dl className="mt-12 grid grid-cols-2 gap-px overflow-hidden rounded-2xl border border-line bg-line sm:grid-cols-3 lg:mt-16 lg:grid-cols-6">
        {[
          ["Orders", number(data.rows.orders)],
          ["Order lines", number(data.rows.lines)],
          ["Events", number(data.rows.events)],
          ["Customers", number(data.rows.customers)],
          ["Products", number(data.rows.products)],
          ["Written in", `${data.rows.seed_seconds} s`],
        ].map(([label, value]) => (
          <div key={label} className="bg-ink-1 px-4 py-4">
            <dt className="text-xs text-fg-3">{label}</dt>
            <dd className="mt-1 font-display text-display-sm font-semibold text-fg numeric">{value}</dd>
          </div>
        ))}
      </dl>
      <p className="mt-3 text-sm text-fg-3">
        The seed writes them in one transaction when the app starts, a chunk of 2,000 orders at a time, and builds the indexes
        after the rows: {(data.rows.bytes / 1024 / 1024).toFixed(0)} MB on disk. {data.method} Measured on {data.machine}.
      </p>

      <div className="mt-10 overflow-x-auto rounded-2xl border border-line">
        <table className="w-full min-w-[44rem] text-left text-[0.9375rem]">
          <caption className="sr-only">First screen and bytes for each page, on a fast machine and on a slow phone</caption>
          <thead className="bg-ink-1 text-sm text-fg-3">
            <tr>
              <th scope="col" className="px-4 py-3 font-medium">Page</th>
              <th scope="col" className="px-4 py-3 text-right font-medium">First screen</th>
              <th scope="col" className="px-4 py-3 text-right font-medium">Slow phone</th>
              <th scope="col" className="px-4 py-3 text-right font-medium">HTML, compressed</th>
              <th scope="col" className="px-4 py-3 text-right font-medium">HTML, uncompressed</th>
              <th scope="col" className="px-4 py-3 text-right font-medium">All, first load</th>
            </tr>
          </thead>
          <tbody>
            {data.pages.map((p) => (
              <tr key={p.name} className="border-t border-line">
                <th scope="row" className="px-4 py-3 font-medium text-fg-1">
                  {p.name} <span className="ml-1 font-mono text-xs font-normal text-fg-3">{p.path.length > 28 ? `${p.path.slice(0, 28)}…` : p.path}</span>
                </th>
                <td className="px-4 py-3 text-right text-fg-2 numeric">{seconds(p.desktop.lcp_ms)}</td>
                <td className="px-4 py-3 text-right text-fg-2 numeric">{seconds(p.phone.lcp_ms)}</td>
                <td className="px-4 py-3 text-right text-fg-2 numeric">{kb(p.desktop.html_bytes)}</td>
                <td className="px-4 py-3 text-right text-fg-2 numeric">{kb(p.desktop.html_decoded_bytes)}</td>
                <td className="px-4 py-3 text-right text-fg-2 numeric">{kb(p.desktop.total_bytes)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="mt-3 text-sm text-fg-3">
        First screen is the largest contentful paint. The order drawer opens {data.drawer.open_ms} ms after the click on one
        small answer of {kb(data.drawer.bytes)}; its history and the customer&apos;s other orders follow by {data.drawer.parts_ms} ms.
      </p>

      <h3 className="mt-14 font-display text-display-sm font-semibold text-fg">What the database does</h3>
      <p className="mt-3 max-w-3xl text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
        Every statement each view runs, from the app&apos;s own query log, with the plan SQLite chose for it. Each list is read
        through an index that ends in the key, so the next page starts where the last one stopped.
      </p>
      <div className="mt-6 grid gap-3 lg:grid-cols-3">
        {data.plans.map((view) => (
          <div key={view.name} className="rounded-2xl border border-line bg-ink-1 p-5">
            <h4 className="font-medium text-fg">{view.name}</h4>
            <ul className="mt-3 space-y-3">
              {view.statements.map((s, i) => (
                <li key={i} className="border-t border-line pt-3 text-sm">
                  <p className="text-fg-2">{readsBy(s.plan)}</p>
                  <details className="mt-1">
                    <summary className="cursor-pointer text-fg-3">The statement, {s.ms} ms</summary>
                    <pre className="scroll-thin mt-2 overflow-x-auto font-mono text-xs leading-relaxed whitespace-pre-wrap text-fg-3">
                      {s.sql}
                    </pre>
                  </details>
                </li>
              ))}
            </ul>
          </div>
        ))}
      </div>
    </Section>
  );
}

function Run() {
  return (
    <Section id="run">
      <SectionHeading
        split
        id="run-heading"
        eyebrow="Run it"
        title="One command, on your machine."
        lead={
          <p>
            The app writes the whole shop when it starts, then a simulator places, packs, ships and delivers orders every few
            seconds. Open it at <code className="inline-code">http://127.0.0.1:8095</code>.
          </p>
        }
      />
      <div className="mt-12 grid gap-3 lg:mt-16 lg:grid-cols-2">
        <div className="grid content-start gap-4">
          <CommandLine command="cargo run --release -p store-demo" comment="the whole shop, live" />
          <p className="text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
            Set <code className="inline-code">STORE_SIMULATOR=off</code> to keep the shop still,{" "}
            <code className="inline-code">PORT=9000</code> for another port, and{" "}
            <code className="inline-code">STORE_DATABASE_URL</code> for another SQLite file. A release build writes the shop in
            about {Math.round(data.rows.seed_seconds)} s.
          </p>
        </div>
        <div className="rounded-2xl border border-line bg-ink-1 p-6 sm:p-8">
          <h3 className="font-display text-display-sm font-semibold text-fg">Read the source</h3>
          <p className="mt-3 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
            The views are in <code className="inline-code">src/views</code>, the rules for refunds, cancels and restocks in{" "}
            <code className="inline-code">src/shop.rs</code>, the seed and the simulator in{" "}
            <code className="inline-code">src/data.rs</code> and <code className="inline-code">src/live.rs</code>. Its
            browser tests are <code className="inline-code">e2e/ui/tests/store.test.mjs</code>.
          </p>
          <div className="mt-6 flex flex-wrap gap-3">
            <SecondaryCta href={repoTree(SOURCE)}>Source on GitHub</SecondaryCta>
            <SecondaryCta href="/ui/#build-anything">More on the UI page</SecondaryCta>
          </div>
        </div>
      </div>
    </Section>
  );
}

export default function StorePage() {
  return (
    <>
      <Hero />
      <Screens />
      <Built />
      <Scale />
      <Run />
      <MoreApps current="store" />
      <PageSchema path="/showcase/store/" metadata={metadata} />
    </>
  );
}
