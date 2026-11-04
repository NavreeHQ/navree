import Link from "next/link";
import type { ReactNode } from "react";

import { ArrowLink, HomeSection, SectionHeading } from "@/components/home/parts";
import { example } from "@/components/ui-docs/data";
import { PreviewFrame } from "@/components/ui-docs/preview-frame";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { highlight } from "@/lib/mdx";
import { cn } from "@/lib/utils";
import { repoBlob, repoTree } from "@site";

import { type Showcase, showcases } from "./showcases";

/** The height a whole-page preview gets on a phone, where it is drawn at the phone's own width. */
const PHONE_HEIGHT = 720;

/** Text with `code` in backticks and `[links](/path/)`, as the showcase data writes it. */
function Rich({ text }: { text: string }): ReactNode {
  const parts = text.split(/(`[^`]+`|\[[^\]]+\]\([^)]+\))/g);
  return parts.map((part, i) => {
    if (part.startsWith("`") && part.endsWith("`") && part.length > 2) {
      return (
        <code key={i} className="inline-code">
          {part.slice(1, -1)}
        </code>
      );
    }
    const link = part.match(/^\[([^\]]+)\]\(([^)]+)\)$/);
    if (link) {
      return (
        <Link key={i} href={link[2]} className="link">
          {link[1]}
        </Link>
      );
    }
    return part;
  });
}

/** The Rust of an example, highlighted at build time, without Shiki's own `<pre>`. */
async function rust(code: string): Promise<string> {
  return (await highlight(code, "rust")).replace(/^<pre[^>]*>/, "").replace(/<\/pre>\s*$/, "");
}

/** One panel of an app's Rust, titled with its file, which links to the file on GitHub. */
async function Code({ code, path }: { code: string; path: string }) {
  const html = await rust(code);
  return (
    <figure className="code-panel flex min-w-0 flex-col">
      <figcaption className="code-block-bar">
        <a href={repoBlob(path)} className="code-block-title hover:text-fg">
          {path}
        </a>
        <span className="code-block-lang font-mono text-[0.8125rem]">Rust</span>
      </figcaption>
      <pre
        className="shiki navree-night scroll-thin max-h-[28rem] overflow-y-auto lg:max-h-[36rem]"
        tabIndex={0}
        // Shiki's output for an example app's own source, read by the UI docs and
        // highlighted at build time (Shiki escapes the code it highlights).
        // nosemgrep: navree-raw-html
        dangerouslySetInnerHTML={{ __html: html }}
      />
    </figure>
  );
}

/** What navree-ui drew from an app's Rust, framed at the full content width, with its caption. */
export function Preview({
  id,
  title,
  caption,
  interactive,
  defer,
}: {
  id: string;
  title: string;
  caption: string;
  interactive?: boolean;
  /** Load the frame only as it nears the screen, after the page has loaded. */
  defer?: boolean;
}) {
  const preview = example(id);
  return (
    <figure className="bezel min-w-0">
      <div className="bezel-core overflow-hidden" inert={!interactive || undefined}>
        <PreviewFrame
          src={preview.src}
          title={title}
          height={preview.height}
          width={preview.width}
          phoneHeight={PHONE_HEIGHT}
          defer={defer}
        />
      </div>
      <figcaption className="px-3 pt-3 pb-1.5 text-sm leading-relaxed text-pretty text-fg-3">
        <Rich text={caption} />
      </figcaption>
    </figure>
  );
}

/** One app: its heading, what to notice, then its views and its code in tabs at the full width. */
async function ShowcaseBlock({ showcase, n }: { showcase: Showcase; n: number }) {
  const source = `apps/${showcase.app}`;
  const code = showcase.views.map((view) => ({ view, code: example(view.example).code }));
  return (
    <article id={showcase.id} aria-labelledby={`${showcase.id}-title`} className="scroll-mt-24 border-t border-line pt-8">
      <div className="grid gap-x-12 gap-y-6 lg:grid-cols-12">
        <div className="lg:col-span-7">
          <p className="font-mono text-xs text-fg-3 numeric">{String(n).padStart(2, "0")}</p>
          <h3 id={`${showcase.id}-title`} className="mt-2 font-display text-display-md font-semibold text-balance text-fg">
            {showcase.title}
          </h3>
          <p className="mt-3 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
            <Rich text={showcase.lead} />
          </p>
        </div>
        <div className="flex flex-col gap-4 lg:col-span-5 lg:pt-6">
          <ul className="space-y-2 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
            {showcase.points.map((point) => (
              <li key={point} className="flex gap-2.5">
                <span aria-hidden className="mt-[0.6em] size-1 shrink-0 rounded-full bg-fg-3" />
                <span>
                  <Rich text={point} />
                </span>
              </li>
            ))}
          </ul>
          <p className="flex flex-wrap items-center gap-x-5 gap-y-2 text-sm">
            <ArrowLink href={repoTree(source)}>Source on GitHub</ArrowLink>
            <code className="inline-code whitespace-nowrap">cargo run -p {showcase.app}</code>
          </p>
        </div>
      </div>

      <Tabs defaultValue={showcase.views[0].example} className="mt-8 gap-4">
        <TabsList
          aria-label={`${showcase.title}: screens and code`}
          className="max-w-full flex-wrap justify-start gap-y-1 rounded-2xl group-data-[orientation=horizontal]/tabs:h-auto sm:rounded-full"
        >
          {showcase.views.map((view) => (
            <TabsTrigger key={view.example} value={view.example} className="px-3.5">
              {view.label}
            </TabsTrigger>
          ))}
          <TabsTrigger value="code" className="px-3.5">
            Code
          </TabsTrigger>
        </TabsList>
        {showcase.views.map((view) => (
          <TabsContent key={view.example} value={view.example} forceMount className="data-[state=inactive]:hidden">
            <Preview id={view.example} title={view.title} caption={view.caption} interactive={view.interactive} />
          </TabsContent>
        ))}
        <TabsContent value="code" forceMount className="data-[state=inactive]:hidden">
          <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
            {code.map(({ view, code }) => (
              <Code key={view.example} code={code} path={`${source}/${view.file}`} />
            ))}
          </div>
        </TabsContent>
      </Tabs>
    </article>
  );
}

/**
 * "Build anything": the example apps in the repository, one after another at the
 * full content width, each read from `showcases.ts`. Every preview is the app's own
 * view, rendered by navree-ui from the app's seed by `cargo run -p ui-docs`, and the
 * Code tab shows the app's source as the UI docs read it. Nothing here is drawn by
 * the site.
 */
export async function BuildAnything() {
  return (
    <HomeSection id="build-anything">
      <SectionHeading
        split
        id="build-anything"
        eyebrow="Build anything"
        title="Whole products, from the same parts."
        lead={
          <p>
            Each example is an app in the repository, drawn by navree-ui from the app&apos;s own Rust: open its Code tab
            to read what drew it, or run it with one command.
          </p>
        }
        action={<ArrowLink href="/showcase/">Every example app</ArrowLink>}
      />
      <div className={cn("mt-12 space-y-16 lg:mt-16 lg:space-y-24")}>
        {showcases.map((showcase, at) => (
          <ShowcaseBlock key={showcase.id} showcase={showcase} n={at + 1} />
        ))}
      </div>
    </HomeSection>
  );
}
