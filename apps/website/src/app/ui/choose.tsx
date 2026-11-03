import Link from "next/link";

import { ArrowLink, HomeSection, SectionHeading } from "@/components/home/parts";
import { InlineMarkdown } from "@/components/inline-markdown";
import { UiChoose } from "@/components/ui-docs";

/** The three ways to draw screens, as the docs' "Choosing a UI" page states them. No ranking: same weight, same fields. */
const options = [
  {
    name: "Navree UI",
    start: "UiModule::from_config()",
    when: "You want one language, one binary and one deploy, and your screens are forms, tables and dashboards.",
    gives:
      "Forms from a validated struct, a server-side DataTable, the app shell, and pages that work with JavaScript off. No Node at run time or in `cargo build`.",
    costs:
      "Interactions that need data go to the server and back, so they wait on the network. Charts and rich text are still to come.",
    href: "/docs/ui/overview/",
    link: "Navree UI overview",
  },
  {
    name: "Next.js (React)",
    start: "navree new <name> --ui next",
    when: "Your team builds in React, or your screens need instant interactions in the browser.",
    gives:
      "React and its libraries. `--ui next` writes a Next.js app in `web/` with its own backend-for-frontend: a sign-in page and route handlers that keep the tokens in httpOnly cookies. Add `@navree/next` for two-factor, a typed client, a `DataTable`, live channels and abilities. The SaaS starter kit ships a full app.",
    costs: "A second app in a second language, with Node.js to run its server and a build step.",
    href: "/docs/frontend/nextjs/",
    link: "Next.js",
  },
  {
    name: "Nuxt (Vue)",
    start: "@navree/nuxt",
    when: "Your team builds in Vue.",
    gives:
      "Vue and its libraries. `@navree/nuxt` is a Nuxt module: sign-in, a session gate, auto-imported composables (`useNavreeAuth`, `useListQuery`, `useChannel`) and `<NavreeDataTable>`.",
    costs:
      "A second app, and Node.js to run it. There is no Nuxt starter kit and no `--ui nuxt`: you create the Nuxt app and add the module.",
    href: "/docs/frontend/nuxt/",
    link: "Nuxt",
  },
];

/** "Choose your front end": the selection diagram, then what each way gives and costs. */
export function ChooseFrontEnd() {
  return (
    <HomeSection id="choose">
      <SectionHeading
        split
        id="choose"
        eyebrow="Choose your front end"
        title="Three ways to draw your screens."
        lead={
          <p>
            Navree UI, Next.js and Nuxt use the same API, the same access rules and the same dark theme, so you can mix them: an
            admin area in Navree UI and a customer app in Next.js, say.
          </p>
        }
        action={<ArrowLink href="/docs/ui/choosing-a-ui/">Choosing a UI, in the docs</ArrowLink>}
      />

      <div className="mx-auto mt-10 max-w-3xl lg:mt-12">
        <UiChoose name="front-end" />
      </div>

      <ul className="mt-6 grid gap-3 md:grid-cols-3">
        {options.map((o) => (
          <li key={o.name} className="flex min-w-0 flex-col rounded-2xl border border-line bg-ink-1 p-6 shadow-bezel">
            <h3 className="font-display text-display-sm font-semibold text-fg">{o.name}</h3>
            <p className="mt-2 font-mono text-[0.8125rem] break-words text-fg-3">{o.start}</p>
            <p className="mt-4 text-[0.9375rem] leading-relaxed text-pretty text-fg-1">{o.when}</p>
            <dl className="mt-5 space-y-4 border-t border-line pt-5 text-sm leading-relaxed text-pretty">
              <div>
                <dt className="font-medium text-fg-1">Gives</dt>
                <dd className="mt-1 text-fg-2">
                  <InlineMarkdown text={o.gives} />
                </dd>
              </div>
              <div>
                <dt className="font-medium text-fg-1">Costs</dt>
                <dd className="mt-1 text-fg-2">
                  <InlineMarkdown text={o.costs} />
                </dd>
              </div>
            </dl>
            <div className="mt-auto pt-6">
              <Link href={o.href} className="link text-sm">
                {o.link}
              </Link>
            </div>
          </li>
        ))}
      </ul>
    </HomeSection>
  );
}
