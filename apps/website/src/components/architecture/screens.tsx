import { ArrowLink, SectionHeading } from "@/components/home/parts";
import { InlineMarkdown } from "@/components/inline-markdown";

/** The three ways to draw an app's screens, the same for every shape. */
const ways = [
  {
    name: "Navree UI",
    code: "navree::ui",
    text: "Typed Rust components that the server renders to HTML, in the same binary as your API, with the rules below built in.",
  },
  {
    name: "Next.js (React)",
    code: "navree new --ui next",
    text: "A Next.js app in `web/` beside the same API, with `@navree/next` for a typed client, a DataTable and live channels.",
  },
  {
    name: "Nuxt (Vue)",
    code: "@navree/nuxt",
    text: "A Nuxt app beside the same API, with the `@navree/nuxt` module for sign-in, data and live updates. It has no starter kit yet.",
  },
];

/** "And the screens": after the six shapes, how any of them draws its screens. */
export function Screens() {
  return (
    <section id="screens" aria-labelledby="screens-heading" className="cv-auto scroll-mt-[calc(var(--header-h)+3.25rem)] border-b border-line">
      <div className="container-site section-y">
        <SectionHeading
          split
          id="screens-heading"
          eyebrow="And the screens"
          title="Whatever the shape, draw its screens your way."
          lead={
            <p>
              Each of these shapes can serve screens as well as an API. Draw them in Rust with Navree UI, or put a Next.js or
              Nuxt app beside the same API, with the same access rules.
            </p>
          }
          action={
            <div className="flex flex-wrap gap-x-6 gap-y-2">
              <ArrowLink href="/ui/">See Navree UI</ArrowLink>
              <ArrowLink href="/docs/ui/choosing-a-ui/">Choosing a UI</ArrowLink>
            </div>
          }
        />
        <dl className="mt-12 grid overflow-hidden rounded-2xl border border-line bg-ink-1/60 md:grid-cols-3 lg:mt-14">
          {ways.map((way, i) => (
            <div key={way.name} className={i === 0 ? "p-6 sm:p-7" : "border-t border-line p-6 sm:p-7 md:border-t-0 md:border-l"}>
              <dt>
                <span className="block text-base font-semibold text-fg">{way.name}</span>
                <code className="mt-1.5 block font-mono text-xs text-fg-3">{way.code}</code>
              </dt>
              <dd className="mt-4 text-[0.9375rem] leading-relaxed text-pretty text-fg-1">
                <InlineMarkdown text={way.text} />
              </dd>
            </div>
          ))}
        </dl>
      </div>
    </section>
  );
}
