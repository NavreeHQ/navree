import Link from "next/link";

import { highlight } from "@/lib/mdx";
import { cn } from "@/lib/utils";

import { example } from "./data";
import { PreviewFrame } from "./preview-frame";

/**
 * One real example from the UI docs, `data-table/default`: its Rust (the
 * example's body, compiled by `apps/ui-docs`) beside the page navree-ui
 * renders from it, framed as a still. Used by the homepage and `/ui/`.
 */
export async function TableShowcase({ className }: { className?: string }) {
  const table = example("data-table/default");
  const html = (await highlight(table.code, "rust")).replace(/^<pre[^>]*>/, "").replace(/<\/pre>\s*$/, "");
  return (
    <div className={cn("grid grid-cols-1 gap-3 lg:grid-cols-12", className)}>
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
          One definition drives the query, the drawing, the CSV export and the bulk actions. Sort, filters and the page live in
          the URL, so a reload or a shared link restores them, and it all works with JavaScript off.
        </p>
      </figure>

      <figure className="bezel min-w-0 lg:col-span-7">
        {/* A still of the docs' preview page: it has no server behind it, so its links and search have nowhere to go. */}
        <div className="bezel-core overflow-hidden" inert>
          <PreviewFrame
            src={table.src}
            title="A data table of deals with search, columns, export and paging, rendered by navree-ui"
            height={table.height}
            width={table.width}
          />
        </div>
        <figcaption className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1 px-3 pt-3 pb-1.5 text-sm leading-relaxed text-fg-3">
          <span>
            Rendered by <span className="font-mono text-[0.8125rem] text-fg-2">navree-ui</span> from that Rust. In your app, the
            search, sort, filters and pages answer from the server.
          </span>
          <Link href="/docs/ui/data-table/" className="link shrink-0">
            DataTable
          </Link>
        </figcaption>
      </figure>
    </div>
  );
}
