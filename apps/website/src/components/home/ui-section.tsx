import Link from "next/link";

import { example, groups } from "@/components/ui-docs/data";
import { PreviewFrame } from "@/components/ui-docs/preview-frame";
import { highlight } from "@/lib/mdx";

import { ArrowLink, HomeSection, SectionHeading } from "./parts";

/** The three ways to draw a Navree app's screens (docs: ui/choosing-a-ui). */
const choices = [
  {
    name: "Navree UI",
    text: "Typed Rust components, rendered on the server in the same binary as your API.",
    meta: "navree::ui",
    href: "/docs/ui/overview/",
  },
  {
    name: "Next.js (React)",
    text: "A React app beside the API, with sign-in, a typed client, a DataTable and live channels.",
    meta: "navree new --ui next",
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
 * navree-ui renders from it (the docs' preview file, live in a frame), then
 * the three ways to draw screens and what is still to come.
 */
export async function UiSection() {
  const table = example("data-table/default");
  const html = (await highlight(table.code, "rust")).replace(/^<pre[^>]*>/, "").replace(/<\/pre>\s*$/, "");
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
        action={<ArrowLink href="/docs/components/overview/">See every component</ArrowLink>}
      />

      <div className="mt-12 grid grid-cols-1 gap-3 lg:mt-14 lg:grid-cols-12">
        <figure className="code-panel flex min-w-0 flex-col lg:col-span-5">
          <figcaption className="code-block-bar">
            <span className="code-block-lang font-mono text-[0.8125rem]">Rust</span>
          </figcaption>
          <pre
            className="shiki navree-night scroll-thin"
            tabIndex={0}
            // Shiki's output for an example compiled by the UI docs, highlighted at build time
            // (Shiki escapes the code it highlights).
            // nosemgrep: navree-raw-html
            dangerouslySetInnerHTML={{ __html: html }}
          />
          <p className="mt-auto border-t border-line px-5 py-4 text-sm leading-relaxed text-pretty text-fg-2">
            One definition drives the query, the drawing, the CSV export and the bulk actions. Sort, filters and the page live
            in the URL, so a reload or a shared link restores them, and it all works with JavaScript off.
          </p>
        </figure>

        <figure className="bezel min-w-0 lg:col-span-7">
          <div className="bezel-core overflow-hidden">
            <PreviewFrame
              src={table.src}
              title="A data table of deals with search, columns, export and paging, rendered by navree-ui"
              height={table.height}
              width={table.width}
            />
          </div>
          <figcaption className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1 px-3 pt-3 pb-1.5 text-sm leading-relaxed text-fg-3">
            <span>
              Rendered by <span className="font-mono text-[0.8125rem] text-fg-2">navree-ui</span> from that Rust. Try the search
              and the columns.
            </span>
            <Link href="/docs/ui/data-table/" className="link shrink-0">
              DataTable
            </Link>
          </figcaption>
        </figure>
      </div>

      <div className="mt-12 grid gap-6 lg:mt-16 lg:grid-cols-12 lg:gap-12">
        <div className="lg:col-span-4">
          <h3 className="font-display text-display-sm font-semibold text-balance text-fg">Or the front end you already use.</h3>
          <p className="mt-3 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
            All three use the same API and the same access rules, so you can mix them: an admin area in Navree UI and a customer
            app in Next.js, say.
          </p>
          <ArrowLink href="/docs/ui/choosing-a-ui/" className="mt-5">
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
        Navree UI is new, and it grows one module at a time. A searchable select, tabs, menus, charts and date pickers are still
        to come, and{" "}
        <Link href="/docs/ui/coming-next/" className="link">
          Coming next
        </Link>{" "}
        lists them in order. Until they land, a Next.js or Nuxt front end beside the same API can draw what Navree UI can&apos;t
        yet.
      </p>
    </HomeSection>
  );
}
