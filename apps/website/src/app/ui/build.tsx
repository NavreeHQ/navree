import Link from "next/link";
import type { ReactNode } from "react";

import { ArrowLink, HomeSection, SectionHeading } from "@/components/home/parts";
import { InlineMarkdown } from "@/components/inline-markdown";
import { example } from "@/components/ui-docs/data";
import { PreviewFrame } from "@/components/ui-docs/preview-frame";
import { highlight } from "@/lib/mdx";
import { cn } from "@/lib/utils";
import { repoTree } from "@site";

/** Where the example app lives in the repository. */
const APP = "apps/campaign-demo";

const OWN_ELEMENTS =
  "The funnel is not a component. It is `el(\"svg\")` and `el(\"rect\")` with the theme's colours as CSS variables, so it follows the theme, writes no `style` attribute and passes the strict content security policy. The app's hourly chart is drawn the same way.";

/** The Rust of an example, highlighted at build time, without Shiki's own `<pre>`. */
async function rust(code: string): Promise<string> {
  return (await highlight(code, "rust")).replace(/^<pre[^>]*>/, "").replace(/<\/pre>\s*$/, "");
}

/** One panel of the app's Rust, with the file it comes from. */
async function Code({ code, file, className }: { code: string; file: string; className?: string }) {
  const html = await rust(code);
  return (
    <figure className={cn("code-panel flex min-w-0 flex-col", className)}>
      <figcaption className="code-block-bar">
        <span className="code-block-title">{file}</span>
        <span className="code-block-lang font-mono text-[0.8125rem]">Rust</span>
      </figcaption>
      <pre
        className="shiki navree-night scroll-thin max-h-[24rem] overflow-y-auto lg:max-h-[34rem]"
        tabIndex={0}
        // Shiki's output for the example app's own source, read by the UI docs and
        // highlighted at build time (Shiki escapes the code it highlights).
        // nosemgrep: navree-raw-html
        dangerouslySetInnerHTML={{ __html: html }}
      />
    </figure>
  );
}

/** What navree-ui drew from that Rust, framed, with a caption under it. */
function Preview({
  id,
  title,
  caption,
  inert = true,
  className,
}: {
  id: string;
  title: string;
  caption: ReactNode;
  /** A still: nothing in it to press. Off for the sheet, whose tabs switch in the preview. */
  inert?: boolean;
  className?: string;
}) {
  const preview = example(id);
  return (
    <figure className={cn("bezel min-w-0", className)}>
      <div className="bezel-core overflow-hidden" inert={inert || undefined}>
        <PreviewFrame src={preview.src} title={title} height={preview.height} width={preview.width} />
      </div>
      <figcaption className="px-3 pt-3 pb-1.5 text-sm leading-relaxed text-pretty text-fg-3">{caption}</figcaption>
    </figure>
  );
}

/** A part of the app: its number, its name and what it shows, then the preview and the Rust. */
function Part({ n, title, text, children }: { n: number; title: string; text: string; children: ReactNode }) {
  return (
    <div className="border-t border-line pt-8">
      <div className="grid gap-3 lg:grid-cols-12 lg:gap-12">
        <p className="font-mono text-xs text-fg-3 numeric lg:col-span-1 lg:pt-1.5">{String(n).padStart(2, "0")}</p>
        <div className="lg:col-span-7">
          <h3 className="font-display text-display-sm font-semibold text-balance text-fg">{title}</h3>
          <p className="mt-2 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
            <InlineMarkdown text={text} />
          </p>
        </div>
      </div>
      <div className="mt-6 lg:mt-8">{children}</div>
    </div>
  );
}

/**
 * "Build anything": an example app in the repository (`apps/campaign-demo`), an
 * email campaign's recipients, a recipient's timeline, its live activity and a
 * funnel. Each preview is the app's own view, rendered by navree-ui from the
 * app's seed by `cargo run -p ui-docs`, and the code beside it is the app's
 * source as the UI docs read it. Nothing here is drawn by the site.
 */
export async function BuildAnything() {
  const table = example("build-anything/recipients");
  const sheet = example("build-anything/recipient");
  const feed = example("build-anything/activity");
  const funnel = example("build-anything/funnel");
  const [row, ...fns] = table.code.split(/\n\n(?=pub fn )/);
  return (
    <HomeSection id="build-anything">
      <SectionHeading
        split
        id="build-anything"
        eyebrow="Build anything"
        title="A whole product, from the same parts."
        lead={
          <p>
            An example app follows one email campaign: a table of its recipients, each recipient&apos;s story in a sheet, a
            feed that moves as people open and click, and a funnel drawn from plain elements. Every picture below is that
            app&apos;s own Rust, rendered by navree-ui.
          </p>
        }
        action={<ArrowLink href={repoTree(APP)}>Read the app&apos;s source</ArrowLink>}
      />

      <div className="mt-12 space-y-14 lg:mt-16 lg:space-y-20">
        <Part
          n={1}
          title="A table that runs on the server"
          text="One struct says what each column can do. The same definition reads the URL, runs the search, the status, place and date filters, the sort and the pages as one query on SQLite, and draws the rows with your own cells: a badge with a dot, a flag and a city. When the campaign moves, a server-sent event tells the table to fetch itself again."
        >
          <Preview
            id={table.id}
            title="A table of a campaign's recipients, with search, filters, status badges, flags and pages, rendered by navree-ui"
            caption={
              <>
                Rendered by <span className="font-mono text-[0.8125rem] text-fg-2">navree-ui</span> from the Rust below. In the
                app, the search, filters, sort and pages answer from the server, and a name opens the sheet underneath.
              </>
            }
          />
          <div className="mt-3 grid grid-cols-1 gap-3 lg:grid-cols-2">
            <Code code={row} file={`${APP}/src/views/recipients.rs`} />
            <Code code={fns.join("\n\n")} file={`${APP}/src/views/recipients.rs`} />
          </div>
        </Part>

        <Part
          n={2}
          title="A record in a sheet, with its timeline"
          text="A row asks the server for the recipient, and the answer fills the sheet and opens it, with no page load; with JavaScript off the name leads to the recipient's own page. Outcome chips, fact cards with a flag, tabs, and a timeline that groups the steps by day, says how long each took, and sets a mail proxy's opens aside until you ask."
        >
          <div className="grid grid-cols-1 gap-3 lg:grid-cols-12">
            <Preview
              id={sheet.id}
              title="A recipient's sheet: outcome chips, fact cards, tabs and a timeline grouped by day, rendered by navree-ui"
              inert={false}
              className="lg:col-span-7"
              caption="The sheet as it opens over the table. Its tabs switch here too."
            />
            <Code code={sheet.code} file={`${APP}/src/views/recipient.rs`} className="lg:col-span-5 lg:self-start" />
          </div>
        </Part>

        <Part
          n={3}
          title="Activity, live"
          text="A feed in a region that follows a server-sent event stream. An event is only a nudge: the feed asks its own server for the rows that are new and adds them at the top, so nothing a stream says can put anything in the page. The live dot follows the connection, and a period select narrows the feed in place."
        >
          <div className="grid grid-cols-1 gap-3 lg:grid-cols-12">
            <Code code={feed.code} file={`${APP}/src/views/dashboard.rs`} className="lg:order-1 lg:col-span-5 lg:self-start" />
            <Preview
              id={feed.id}
              title="A live feed of what recipients did: opens, clicks and trials, each with a device and a flag, rendered by navree-ui"
              className="lg:order-2 lg:col-span-7"
              caption="Replayed in your browser: the rows were drawn by the app's Rust ahead of time. In your app the server streams them over SSE."
            />
          </div>
        </Part>

        <Part
          n={4}
          title="Your own elements, when no component fits"
          text={OWN_ELEMENTS}
        >
          <div className="grid grid-cols-1 gap-3 lg:grid-cols-12">
            <Preview
              id={funnel.id}
              title="A funnel of four bars drawn with plain SVG elements, rendered by navree-ui"
              className="lg:col-span-7 lg:self-start"
              caption="Plain elements, drawn on the server: no script and no chart library."
            />
            <Code code={funnel.code} file={`${APP}/src/views/dashboard.rs`} className="lg:col-span-5 lg:self-start" />
          </div>
        </Part>
      </div>

      <p className="mt-12 max-w-3xl text-[0.9375rem] leading-relaxed text-pretty text-fg-3 lg:mt-16">
        The app also edits a contact: a{" "}
        <Link href="/docs/components/phone-input/" className="link">
          phone number
        </Link>{" "}
        read in the country chosen beside it, a country found by typing with its{" "}
        <Link href="/docs/ui/flags/" className="link">
          flag
        </Link>
        , a{" "}
        <Link href="/docs/components/date-picker/" className="link">
          date
        </Link>{" "}
        typed or picked, and a{" "}
        <Link href="/docs/components/toast/" className="link">
          toast
        </Link>{" "}
        on save. Run it with <code className="inline-code">cargo run -p campaign-demo</code>, or{" "}
        <a href={repoTree(APP)} className="link">
          read its source
        </a>
        .
      </p>
    </HomeSection>
  );
}
