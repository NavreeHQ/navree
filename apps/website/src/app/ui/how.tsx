import { CheckIcon } from "lucide-react";

import { DiagramFigure, diagrams } from "@/components/diagrams";
import { ArrowLink, HomeSection, SectionHeading } from "@/components/home/parts";
import { InlineMarkdown } from "@/components/inline-markdown";
import { kb, toolkitInfo } from "@/components/ui-docs/data";
import { uiBudgets } from "@/lib/ui-page-data";

/** How Navree UI works, as the overview page states it. */
const principles = [
  {
    term: "One typed tree",
    text: "Every component is a struct with builder methods that turns into one `Node`. Variants are Rust enums, so a misspelt variant doesn't compile, and `render` writes the tree in one pass.",
  },
  {
    term: "One attribute contract",
    text: "`data-slot` says what a part is, `data-variant` and `data-size` are closed choices, and `data-open` or `data-invalid` are states. The stylesheet and the runtime read nothing else.",
  },
  {
    term: "The browser's own elements",
    text: "Dialogs are `<dialog>`, accordions `<details>`, popovers `popover`, switches checkboxes. They work with no script.",
  },
  {
    term: "Actions answer with ops",
    text: "A form or a command posts to a handler, which answers with a `Reply`: ops for the runtime, or a redirect or a full page when JavaScript is off.",
  },
];

/** What keeps working with JavaScript off (docs: ui/overview, forms, data-table, app-shell). */
const noScript = [
  "Pages and links",
  "Forms: they post, and answer with the page again, errors under the inputs",
  "Dialogs, accordions and switches",
  "The DataTable: sort, filter and page, through the URL",
  "The user menu's actions, with the session's CSRF token",
];

/** One budget: the measured size against the ceiling a test holds it to. */
function Meter({ label, measured, ceiling }: { label: string; measured: number; ceiling: number }) {
  const share = Math.min(100, Math.round((measured / (ceiling * 1024)) * 100));
  return (
    <div>
      <div className="flex items-baseline justify-between gap-3 text-sm">
        <span className="text-fg-1">{label}</span>
        <span className="shrink-0 font-mono text-xs text-fg-3 numeric">
          <span className="text-fg">{kb(measured)}</span> of {ceiling} KB
        </span>
      </div>
      <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-ink-3" aria-hidden>
        <div className="h-full rounded-full bg-accent/70" style={{ width: `${share}%` }} />
      </div>
    </div>
  );
}

/** "How it works": the diagram, the four ideas, then the browser's side: size, security and JavaScript off. */
export function HowItWorks() {
  const gzip = (name: string) => toolkitInfo.assets.find((a) => a.name === name)?.gzip ?? 0;
  const scripts = toolkitInfo.assets.filter((a) => a.ext === "js").map((a) => a.name);
  const budgets = uiBudgets().map((b) => ({
    ...b,
    measured: (b.files === "js" ? scripts : b.files).reduce((n, f) => n + gzip(f), 0),
  }));
  const csp = toolkitInfo.csp.split(/;\s*/).filter(Boolean);
  return (
    <HomeSection id="how">
      <SectionHeading
        split
        id="how"
        eyebrow="How it works"
        title="Typed Rust in, HTML out."
        lead={
          <p>
            A handler returns components. Navree UI renders them to HTML on the server, and the browser gets a page that works
            before any script runs. Script loads only for what the page&apos;s markup uses.
          </p>
        }
        action={<ArrowLink href="/docs/ui/overview/">How Navree UI works</ArrowLink>}
      />

      <div className="mt-12 grid grid-cols-1 gap-x-12 gap-y-10 lg:mt-14 lg:grid-cols-12">
        <div className="min-w-0 lg:col-span-7">
          <DiagramFigure spec={diagrams["navree-ui"]} name="navree-ui" fill maxScale={1.15} />
        </div>
        <dl className="divide-y divide-line border-y border-line lg:col-span-5 lg:self-center">
          {principles.map((p) => (
            <div key={p.term} className="py-4">
              <dt className="text-[0.9375rem] font-semibold text-fg">{p.term}</dt>
              <dd className="mt-1.5 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
                <InlineMarkdown text={p.text} />
              </dd>
            </div>
          ))}
        </dl>
      </div>

      <div className="mt-3 grid grid-cols-1 gap-3 md:grid-cols-2 lg:mt-12 xl:grid-cols-3">
        <section aria-labelledby="ui-weight" className="flex flex-col rounded-2xl border border-line bg-ink-1 p-6 shadow-bezel">
          <h3 id="ui-weight" className="font-display text-display-sm font-semibold text-fg">
            What reaches the browser
          </h3>
          <p className="mt-2 text-sm leading-relaxed text-pretty text-fg-2">
            Gzip sizes, measured from the toolkit&apos;s files. Each ceiling is a test: a change that crosses one fails the build.
          </p>
          <div className="mt-6 space-y-4">
            {budgets.map((b) => (
              <Meter key={b.label} label={b.label} measured={b.measured} ceiling={b.ceilingKb} />
            ))}
          </div>
          <p className="mt-auto pt-6 text-sm leading-relaxed text-fg-3">
            A page with nothing interactive has no <code className="inline-code">&lt;script&gt;</code> at all.
          </p>
        </section>

        <section aria-labelledby="ui-csp" className="flex flex-col rounded-2xl border border-line bg-ink-1 p-6 shadow-bezel">
          <h3 id="ui-csp" className="font-display text-display-sm font-semibold text-fg">
            A strict content security policy
          </h3>
          <p className="mt-2 text-sm leading-relaxed text-pretty text-fg-2">
            Every page carries it: its own origin only, no inline script or style, no framing.
          </p>
          <pre className="mt-5 overflow-x-auto rounded-xl border border-line bg-ink-0 px-4 py-3 font-mono text-xs leading-6 text-fg-2">
            {csp.map((rule) => (
              <span key={rule} className="block">
                {rule};
              </span>
            ))}
          </pre>
          <p className="mt-auto pt-6 text-sm leading-relaxed text-pretty text-fg-3">
            Text goes through one escaper, every URL through <code className="inline-code">Href</code>, and a redirect can&apos;t
            leave the site.
          </p>
        </section>

        <section
          aria-labelledby="ui-no-js"
          className="flex flex-col rounded-2xl border border-line bg-ink-1 p-6 shadow-bezel md:col-span-2 xl:col-span-1"
        >
          <h3 id="ui-no-js" className="font-display text-display-sm font-semibold text-fg">
            With JavaScript off
          </h3>
          <p className="mt-2 text-sm leading-relaxed text-pretty text-fg-2">
            Without the runtime, these still work:
          </p>
          <ul className="mt-5">
            {noScript.map((item) => (
              <li key={item} className="flex gap-3 border-t border-line py-2.5 text-[0.9375rem] leading-snug text-pretty text-fg-1">
                <CheckIcon aria-hidden className="mt-[0.2rem] size-4 shrink-0 text-accent/80" strokeWidth={2} />
                <span>{item}</span>
              </li>
            ))}
          </ul>
        </section>
      </div>
    </HomeSection>
  );
}
