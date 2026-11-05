import { CheckIcon, ChevronRightIcon } from "lucide-react";
import type { Metadata } from "next";
import Link from "next/link";
import type { ReactNode } from "react";

import { ArrowLink, PrimaryCta, SecondaryCta, SectionHeading } from "@/components/home/parts";
import { CodePanel, CommandLine } from "@/components/showcase/code-panel";
import { MoreApps } from "@/components/showcase/more-apps";
import { inboxNumbers as numbers } from "@/components/showcase/inbox-numbers";
import { example } from "@/components/ui-docs/data";
import { PreviewFrame } from "@/components/ui-docs/preview-frame";
import { pageMetadata } from "@/lib/metadata";
import { PageSchema } from "@/lib/structured-data";
import { cn } from "@/lib/utils";
import { repoTree } from "@site";

const APP = "apps/inbox-demo";

export const metadata: Metadata = pageMetadata({
  title: "A support inbox with 100,000 conversations",
  description:
    "An example app on Navree UI: a support inbox in three resizable panes, live over server-sent events, proven at 100,000 conversations and 5.5 million messages.",
  path: "/showcase/inbox/",
});

/** A section of this page: a hairline on top, the container and the rhythm. */
function Section({ id, children, className }: { id: string; children: ReactNode; className?: string }) {
  return (
    <section id={id} aria-labelledby={`${id}-heading`} className={cn("cv-auto scroll-mt-(--header-h) border-t border-line", className)}>
      <div className="container-site section-y">{children}</div>
    </section>
  );
}

/** A sub-section heading inside "How it is built". */
function ChapterHeading({ label, title, children }: { label: string; title: string; children: ReactNode }) {
  return (
    <div>
      <p className="font-mono text-xs tracking-[0.12em] text-fg-3 uppercase">{label}</p>
      <h3 className="mt-3 font-display text-display-md font-semibold text-balance text-fg">{title}</h3>
      <div className="mt-4 max-w-2xl space-y-4 text-[0.9375rem] leading-relaxed text-pretty text-fg-2 sm:text-base">{children}</div>
    </div>
  );
}

/** A list with green checks. */
function Checks({ items, className }: { items: ReactNode[]; className?: string }) {
  return (
    <ul className={cn("mt-6", className)}>
      {items.map((item, i) => (
        <li key={i} className="flex gap-3 border-t border-line py-2.5 text-[0.9375rem] leading-snug text-pretty text-fg-1">
          <CheckIcon aria-hidden className="mt-[0.2rem] size-4 shrink-0 text-accent/80" strokeWidth={2} />
          <span>{item}</span>
        </li>
      ))}
    </ul>
  );
}

const Code = ({ children }: { children: ReactNode }) => <code className="inline-code">{children}</code>;

/** One of the app's screens, as navree-ui drew it from the app's own views. */
function Screen({ id, title, caption, className }: { id: string; title: string; caption: ReactNode; className?: string }) {
  const preview = example(id);
  return (
    <figure className={cn("bezel min-w-0", className)}>
      <div className="bezel-core overflow-hidden" inert>
        <PreviewFrame src={preview.src} title={title} height={preview.height} width={preview.width} fit={false} />
      </div>
      <figcaption className="px-3 pt-3 pb-1.5 text-sm leading-relaxed text-pretty text-fg-3">{caption}</figcaption>
    </figure>
  );
}

/** A top-level item of an example's code: from the line that starts `start` to its closing brace. */
function excerpt(id: string, start: string): string {
  const lines = example(id).code.split("\n");
  let at = lines.findIndex((line) => line.startsWith(start));
  if (at < 0) return "";
  while (at > 0 && (lines[at - 1].startsWith("///") || lines[at - 1].startsWith("#["))) at--;
  const end = lines.findIndex((line, i) => i > at && line === "}");
  return lines.slice(at, end + 1).join("\n");
}

const ms = (n: number) => `${Math.round(n)} ms`;
const kb = (n: number) => `${(n / 1024).toFixed(1)} KB`;
const mb = (kib: number) => (kib / 1024).toFixed(1);
const count = (n: number) => n.toLocaleString("en-US");

/* ------------------------------------------------------------------ hero */

function Hero() {
  const facts = [
    { value: "100,000", label: "conversations", text: "Of 10 to 100 messages each, on SQLite" },
    { value: numbers.messages, label: "messages", text: "Written by the app's own seed, the same on every run" },
    { value: ms(numbers.firstScreen.largest_paint_ms), label: "to the first screen", text: `${kb(numbers.firstScreen.bytes_total)} sent, the median of ${numbers.firstScreen.runs} cold loads` },
    { value: count(numbers.flat.maxNodes), label: "nodes at most", text: `The most the page held while scrolling from ${count(numbers.flat.heldFrom)} to ${count(numbers.flat.heldTo)} conversations` },
  ];
  return (
    <section aria-labelledby="inbox-heading" className="relative isolate overflow-hidden">
      <div className="container-site pt-10 sm:pt-14 lg:pt-16">
        <nav aria-label="Breadcrumb" className="mb-10 lg:mb-14">
          <ol className="flex items-center gap-1.5 font-mono text-xs text-fg-3">
            <li>
              <Link href="/showcase/" className="rounded-sm transition-colors hover:text-fg">
                Showcase
              </Link>
            </li>
            <li aria-hidden>
              <ChevronRightIcon className="size-3.5" />
            </li>
            <li aria-current="page" className="text-fg-2">
              Support inbox
            </li>
          </ol>
        </nav>
        <div className="grid gap-x-12 gap-y-8 lg:grid-cols-12 lg:items-end">
          <div className="lg:col-span-7">
            <p className="eyebrow">Example app</p>
            <h1 id="inbox-heading" className="mt-5 text-display-xl font-semibold text-balance text-fg">
              <span className="block">A support inbox,</span>
              <span className="block text-fg-2">at 100,000 conversations.</span>
            </h1>
          </div>
          <div className="lg:col-span-5 lg:pb-2">
            <p className="max-w-xl text-lg leading-relaxed text-pretty text-fg-2">
              The help desk of an invented Mohali software company, whose booking software runs clinics, studios and
              academies across India: conversations, a thread with notes and files, and the customer beside it, in three
              panes you resize. New messages arrive while you type. Every list loads as you read it, so the first screen
              costs one page whatever the desk holds.
            </p>
            <div className="mt-8 flex flex-wrap items-center gap-3">
              <PrimaryCta href="#run">Run it locally</PrimaryCta>
              <SecondaryCta href={repoTree(APP)}>Source on GitHub</SecondaryCta>
            </div>
          </div>
        </div>
        <div className="mt-12 sm:mt-16 lg:mt-20">
          <Screen
            id="build-anything/inbox-panes"
            title="The support inbox: the conversations, an open conversation with an internal note and a file, and the customer's plan, history and tags, rendered by navree-ui"
            caption="The app's own views, rendered by navree-ui from its seed. In the app, new conversations and messages arrive over server-sent events."
          />
        </div>
      </div>
      <div className="container-site mt-16 lg:mt-24">
        <dl className="grid grid-cols-2 border-t border-line lg:grid-cols-4">
          {facts.map((f, i) => (
            <div
              key={f.label}
              className={cn("py-7 pr-4 sm:py-9 sm:pr-6", i % 2 === 1 && "border-l border-line pl-4 sm:pl-6", i >= 2 && "border-t border-line lg:border-t-0", i === 2 && "lg:border-l lg:pl-6")}
            >
              <dt className="sr-only">{f.label}</dt>
              <dd>
                <span className="block font-display text-display-md font-semibold text-fg numeric">{f.value}</span>
                <span className="mt-1 block text-[0.9375rem] font-medium text-fg-1">{f.label}</span>
                <span className="mt-1 block text-[0.8125rem] leading-snug text-pretty text-fg-3">{f.text}</span>
              </dd>
            </div>
          ))}
        </dl>
      </div>
    </section>
  );
}

/* --------------------------------------------------------------- screens */

function Screens() {
  return (
    <Section id="screens">
      <SectionHeading
        split
        id="screens-heading"
        eyebrow="The screens"
        title="Three panes on a desk, one at a time on a phone."
        lead={<p>Each picture is the app&apos;s own Rust, rendered by navree-ui from the same data the app seeds. Nothing here is drawn by this site.</p>}
      />
      <div className="mt-12 grid gap-6 lg:mt-16">
        <Screen
          id="build-anything/inbox-thread"
          title="A conversation of 100 messages opened at its newest thirty, with a link to earlier messages at the top, rendered by navree-ui"
          caption="A conversation of 100 messages opens at its newest thirty. Older ones load as you scroll up, and the message you were reading stays where it was. Replayed in your browser: in the app each page comes from the server by cursor."
        />
        <div className="grid gap-6 lg:grid-cols-12">
          <Screen
            id="build-anything/inbox-phone"
            className="lg:col-span-4"
            title="The inbox on a phone: one pane at a time, with a link back to the conversations, rendered by navree-ui"
            caption="On a phone the panes show one at a time, with links between them."
          />
          <Screen
            id="build-anything/inbox-palette"
            className="lg:col-span-8"
            title="The command palette: actions on the open conversation, saved replies and places to go, rendered by navree-ui"
            caption="Ctrl+K: assign, close, snooze and change the priority, or drop a saved reply into what you are writing."
          />
        </div>
      </div>
    </Section>
  );
}

/* ---------------------------------------------------------- how it works */

function Built() {
  const panes = "build-anything/inbox-panes";
  return (
    <Section id="built">
      <SectionHeading
        split
        id="built-heading"
        eyebrow="How it is built"
        title="Server-rendered Rust, small modules in the browser."
        lead={
          <p>
            Every pane is drawn on the server. In the browser, the core runtime and {numbers.firstScreen.scripts.length - 1}{" "}
            modules do the rest on the first screen: the split, the lazy lists, the live stream, the keys, the sidebar, the
            select and the overlays, {kb(numbers.firstScreen.script_bytes)} of script compressed. A conversation adds the
            menus and the date picker. With JavaScript off, every action is a link or a form.
          </p>
        }
        action={<ArrowLink href={repoTree(APP)}>Read the app&apos;s source</ArrowLink>}
      />

      <div className="mt-14 grid items-start gap-x-16 gap-y-10 lg:mt-20 xl:grid-cols-12">
        <div className="min-w-0 xl:col-span-5">
          <ChapterHeading label="01 · Panes" title="A split you resize, kept for each agent.">
            <p>
              <Link href="/docs/components/split-pane/" className="link">SplitPane</Link> lays out the three panes. Drag a
              separator, or focus it and use the arrow keys; Enter collapses the customer pane. The sizes are posted to
              the server and drawn into the next page, so nothing jumps.
            </p>
          </ChapterHeading>
          <Checks
            items={[
              <>The WAI-ARIA window splitter: arrows, Shift, Home, End and Enter, with the value read out.</>,
              <>Below 768 px of its own width the split shows one pane at a time, with links back.</>,
              <>
                <Code>SplitSave</Code> answers forgery as a form does; <Code>SplitSizes</Code> holds a save to the layout&apos;s limits.
              </>,
            ]}
          />
        </div>
        <CodePanel file={`${APP}/src/views/inbox.rs`} lang="rust" source={excerpt(panes, "pub fn layout")} className="xl:col-span-7" />
      </div>

      <div className="mt-20 grid items-start gap-x-16 gap-y-10 border-t border-line pt-16 lg:mt-28 lg:pt-20 xl:grid-cols-12">
        <div className="min-w-0 xl:col-span-5">
          <ChapterHeading label="02 · Lazy lists" title="Fifty conversations at a time, never by offset.">
            <p>
              <Link href="/docs/components/lazy-list/" className="link">LazyList</Link> draws the first page and asks for the
              next by the cursor of the last row: <Code>(last_at, id) &lt; (?, ?)</Code> on an index, so page 400 costs what
              page 1 does, under a channel or priority filter too: each has an index of its own in every list. A search
              reads the full-text index, then sorts what it found. Past 300 rows the far end leaves the page and comes back
              when you scroll to it.
            </p>
          </ChapterHeading>
          <Checks
            items={[
              <>A conversation opens at its newest 30 messages; older ones load above, keeping your place.</>,
              <>Opening a row deep in the list keeps the list and where you scrolled.</>,
              <>Search runs on a full-text index of subjects, names and companies.</>,
            ]}
          />
        </div>
        <CodePanel file={`${APP}/src/queries.rs`} lang="rust" source={excerpt(panes, "pub async fn list_page")} className="xl:col-span-7" />
      </div>

      <div className="mt-20 grid items-start gap-x-16 gap-y-10 border-t border-line pt-16 lg:mt-28 lg:pt-20 xl:grid-cols-12">
        <div className="min-w-0 xl:col-span-5">
          <ChapterHeading label="03 · Live, and checked by the server" title="New messages land where you are looking.">
            <p>
              One server-sent event stream per page says what moved. The list asks what changed since its revision, and
              the server sends only those rows: a conversation with a new message moves to the top, once, and a row
              changes where it stands, so the row you are on keeps your focus. The open thread asks for messages newer
              than its last, and reads out what arrived. Every write takes its revision inside its own transaction, so
              no reader can skip one. The clocks count down on the server.
            </p>
          </ChapterHeading>
          <Checks
            items={[
              <>Assigning searches the team on the server, and the save checks the agent is one you may pick.</>,
              <>A snooze day, a status and a priority are checked again on the server, with JavaScript off too.</>,
              <>
                <Code>j</Code>, <Code>k</Code>, <Code>e</Code>, <Code>r</Code> and <Code>/</Code> never fire while you type;{" "}
                <Code>e</Code> closes only the conversation you are in, and each agent can turn single keys off.
              </>,
            ]}
          />
        </div>
        <CodePanel file={`${APP}/src/views/list.rs`} lang="rust" source={excerpt(panes, "pub fn list_lazy")} className="xl:col-span-7" />
      </div>
    </Section>
  );
}

/* ---------------------------------------------------------------- scale */

function Scale() {
  const times = Object.entries(numbers.server);
  return (
    <Section id="scale">
      <SectionHeading
        split
        id="scale-heading"
        eyebrow="Proven at scale"
        title="100,000 conversations, 5.5 million messages, measured."
        lead={
          <p>
            Measured on one developer machine, release build, SQLite, Chromium. Past {numbers.flat.maxRows} rows the list
            lets the far end go, so the page weighs the same at 2,000 conversations as at {count(numbers.flat.heldFrom)}.
            Every query reads by an index that matches what it filters on, with any channel or priority filter, and the
            suite fails a plan that walks a list.
          </p>
        }
      />
      <div className="mt-12 grid gap-6 lg:mt-16 lg:grid-cols-2">
        <dl className="grid grid-cols-2 gap-px overflow-hidden rounded-[1.25rem] border border-line bg-line">
          {[
            ["First screen of the inbox", `${ms(numbers.firstScreen.largest_paint_ms)} · ${kb(numbers.firstScreen.bytes_total)}`],
            ["Opening a 100-message conversation", `${ms(numbers.openThread.largest_paint_ms)} · ${kb(numbers.openThread.bytes_total)}`],
            ["Scrolling to its oldest message", `${ms(numbers.oldest.ms)} · ${numbers.oldest.fetches} fetches`],
            [`Nodes, ${count(numbers.flat.heldFrom)} to ${count(numbers.flat.heldTo)} conversations scrolled`, `${count(numbers.flat.minNodes)} to ${count(numbers.flat.maxNodes)}`],
            [`JS heap, ${count(numbers.flat.heldFrom)} to ${count(numbers.flat.heldTo)} conversations scrolled`, `${mb(numbers.flat.minHeapKb)} to ${mb(numbers.flat.maxHeapKb)} MB`],
            ["A live answer after one change", `${numbers.live.change.rows} row · ${count(numbers.live.change.bytes)} bytes`],
            ["Seeding the desk", numbers.seed],
          ].map(([label, value]) => (
            <div key={label} className="bg-ink-1 p-5">
              <dt className="text-[0.8125rem] text-fg-3">{label}</dt>
              <dd className="mt-1 font-display text-xl font-semibold text-fg numeric">{value}</dd>
            </div>
          ))}
        </dl>
        <div className="overflow-x-auto rounded-[1.25rem] border border-line">
          <table className="w-full text-left text-[0.875rem]">
            <caption className="sr-only">Server times at full size, 200 requests each, four at a time</caption>
            <thead className="text-fg-3">
              <tr>
                <th scope="col" className="px-4 py-3 font-medium">Request</th>
                <th scope="col" className="px-4 py-3 text-right font-medium">p50</th>
                <th scope="col" className="px-4 py-3 text-right font-medium">p95</th>
              </tr>
            </thead>
            <tbody>
              {times.map(([label, t]) => (
                <tr key={label} className="border-t border-line text-fg-1">
                  <th scope="row" className="px-4 py-2.5 font-normal">{label}</th>
                  <td className="px-4 py-2.5 text-right numeric">{ms(t.p50_ms)}</td>
                  <td className="px-4 py-2.5 text-right numeric">{ms(t.p95_ms)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
      <CodePanel file="EXPLAIN QUERY PLAN" lang="sh" source={numbers.plans} className="mt-6" />
    </Section>
  );
}

/* ------------------------------------------------------------------ run */

function Run() {
  return (
    <Section id="run">
      <SectionHeading
        id="run-heading"
        eyebrow="Run it"
        title="One command, then open the inbox."
        lead={
          <p>
            The app writes its desk into SQLite when it starts and serves it at port 8096. A simulator makes customers and
            teammates act every few seconds.
          </p>
        }
      />
      <div className="mt-10 grid max-w-3xl gap-3">
        <CommandLine command="cargo run --release -p inbox-demo" comment="100,000 conversations" />
        <CommandLine command="INBOX_SCALE=2000 cargo run -p inbox-demo" comment="a small desk, quicker to start" />
      </div>
      <p className="mt-8 max-w-3xl text-[0.9375rem] leading-relaxed text-pretty text-fg-3">
        Then open <Code>http://127.0.0.1:8096/inbox</Code>. Press <Code>?</Code> for the keys. The source is in{" "}
        <a href={repoTree(APP)} className="link">
          {APP}
        </a>
        .
      </p>
      <p className="mt-4 max-w-3xl text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
        <strong className="font-medium text-fg-1">The example has no sign-in.</strong> A visitor works as one of the team,
        named by a cookie the server signs, and can switch from the account menu. A real app puts the inbox behind{" "}
        <Link href="/docs/security/authentication/" className="link">
          navree-auth
        </Link>{" "}
        and checks on every route that the person may see and change the conversation it names.
      </p>
    </Section>
  );
}

export default function InboxShowcase() {
  return (
    <>
      <PageSchema path="/showcase/inbox/" metadata={metadata} />
      <Hero />
      <Screens />
      <Built />
      <Scale />
      <Run />
      <MoreApps current="inbox" />
    </>
  );
}
