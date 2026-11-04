import Link from "next/link";

import { groups } from "@/components/ui-docs/data";
import { TableShowcase } from "@/components/ui-docs/table-showcase";

import { ArrowLink, HomeSection, SectionHeading } from "./parts";

/** The three ways to draw a Navree app's screens (docs: ui/choosing-a-ui). */
const choices = [
  {
    name: "Navree UI",
    text: "Typed Rust components, rendered on the server in the same binary as your API.",
    meta: "navree::ui",
    href: "/ui/",
  },
  {
    name: "Next.js (React)",
    text: "A React app beside the API. The starter signs users in through its own backend-for-frontend; @navree/next adds a typed client, a DataTable and live channels.",
    meta: "navree new <name> --ui next",
    href: "/docs/frontend/nextjs/",
  },
  {
    name: "Nuxt (Vue)",
    text: "A Vue app beside the API, with a Nuxt module for sign-in, composables and a data table.",
    meta: "@navree/nuxt",
    href: "/docs/frontend/nuxt/",
  },
];

/**
 * "Navree UI": one real example from the UI docs, its Rust beside the page
 * navree-ui renders from it (the docs' preview file, framed), then
 * the three ways to draw screens and what is still to come. Its links lead
 * to `/ui/`, the page about the toolkit.
 */
export function UiSection() {
  const count = groups.reduce((n, g) => n + g.components.length, 0);
  return (
    <HomeSection id="navree-ui">
      <SectionHeading
        split
        id="navree-ui"
        eyebrow="Navree UI"
        title="Screens in Rust, rendered on the server."
        lead={
          <p>
            A handler returns typed components, and Navree UI turns them into HTML with one stylesheet, a small runtime that
            loads only what a page uses, and a strict content security policy. {count} components today, each with a live
            preview in the docs.
          </p>
        }
        action={<ArrowLink href="/ui/">Explore Navree UI</ArrowLink>}
      />

      <TableShowcase className="mt-12 lg:mt-14" />

      <p className="mt-6 flex flex-wrap items-baseline gap-x-4 gap-y-2 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
        <span>
          Build anything from the same parts: an example app follows an email campaign, with a recipient&apos;s timeline in a
          sheet and a feed that moves live.
        </span>
        <ArrowLink href="/ui/#build-anything">See the example app</ArrowLink>
      </p>

      <div className="mt-12 grid gap-6 lg:mt-16 lg:grid-cols-12 lg:gap-12">
        <div className="lg:col-span-4">
          <h3 className="font-display text-display-sm font-semibold text-balance text-fg">Or the front end you already use.</h3>
          <p className="mt-3 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
            All three use the same API and the same access rules, so you can mix them: an admin area in Navree UI and a customer
            app in Next.js, say.
          </p>
          <ArrowLink href="/ui/#choose" className="mt-5">
            Choosing a UI
          </ArrowLink>
        </div>
        <ul className="grid gap-px overflow-hidden rounded-2xl border border-line bg-line md:grid-cols-3 lg:col-span-8">
          {choices.map((c) => (
            <li key={c.name} className="bg-ink-0">
              <Link href={c.href} className="group flex h-full flex-col p-5 transition-colors duration-200 hover:bg-ink-1 sm:p-6">
                <span className="text-[0.9375rem] font-semibold text-fg transition-colors group-hover:text-accent">{c.name}</span>
                <span className="mt-2 text-sm leading-relaxed text-pretty text-fg-2">{c.text}</span>
                <span className="mt-auto pt-4 font-mono text-xs text-fg-3">{c.meta}</span>
              </Link>
            </li>
          ))}
        </ul>
      </div>

      <p className="mt-8 max-w-3xl text-[0.9375rem] leading-relaxed text-pretty text-fg-3">
        Navree UI is new, and it grows module by module. Charts are still
        to come, and{" "}
        <Link href="/ui/#coming-next" className="link">
          Coming next
        </Link>{" "}
        lists them in order. Until they land, a Next.js or Nuxt front end beside the same API can draw what Navree UI can&apos;t
        yet.
      </p>
    </HomeSection>
  );
}
