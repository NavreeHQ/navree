import fs from "node:fs";
import path from "node:path";

import Link from "next/link";

import { ArrowLink, HomeSection, SectionHeading } from "@/components/home/parts";
import { CodePanel, CommandLine } from "@/components/showcase/code-panel";
import { BrowserFrame, PhoneFrame } from "@/components/showcase/frames";
import { Preview } from "@/components/showcase/preview";
import { RichText } from "@/components/showcase/rich-text";
import { ShowcaseImage } from "@/components/showcase/showcase-image";
import { example } from "@/components/ui-docs/data";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cut, excerpt } from "@/lib/excerpt";
import { cn } from "@/lib/utils";
import { repoTree } from "@site";

import { type AppCard, appCards, type HardPart, hardParts, type Showcase, showcases } from "./showcases";

/** `2026-11-04` as `4 November 2026`. */
const longDate = (iso: string) =>
  new Date(`${iso}T12:00:00Z`).toLocaleDateString("en-IN", { day: "numeric", month: "long", year: "numeric", timeZone: "UTC" });

/** The cards at the top: every app, its picture, its hardest fact, and the ways to it. */
function AppIndex({ cards }: { cards: AppCard[] }) {
  // Two cards lie on their side from `lg`; three or more stand up, four across at `xl`.
  const wide = cards.length <= 2;
  const blocks = new Set(showcases.map((s) => s.id));
  return (
    <nav aria-label="Example apps" className="mt-12 lg:mt-14">
      <ul className={cn("grid grid-cols-2 gap-3", wide ? "lg:gap-4" : "xl:grid-cols-4")}>
        {cards.map((card) => (
          <li key={card.id} className="min-w-0">
            <article
              className={cn(
                "flex h-full flex-col overflow-hidden rounded-2xl border border-line bg-ink-1 shadow-bezel transition-colors duration-200 hover:border-line-2",
                wide && "lg:flex-row",
              )}
            >
              <div
                className={cn(
                  "aspect-[16/10] shrink-0 overflow-hidden border-b border-line bg-ink-0",
                  wide && "lg:aspect-auto lg:w-[46%] lg:border-r lg:border-b-0",
                )}
              >
                <ShowcaseImage
                  name={card.still}
                  sizes={wide ? "(min-width: 1440px) 315px, (min-width: 1024px) 22vw, 46vw" : "(min-width: 1280px) 330px, 46vw"}
                  className="size-full object-cover object-top-left"
                />
              </div>
              <div className="flex min-w-0 flex-1 flex-col p-3.5 sm:p-5 lg:p-6">
                <p className="font-mono text-xs text-fg-3">{card.ui}</p>
                <p className="mt-2 font-display text-[1.0625rem] leading-snug font-semibold text-balance text-fg sm:text-display-sm">
                  {card.name}
                </p>
                <p className="mt-0.5 text-sm text-fg-3">{card.kind}</p>
                <p className="mt-3 text-[0.8125rem] leading-relaxed text-pretty text-fg-2 sm:text-[0.9375rem]">{card.hardest}</p>
                <p className="mt-auto flex flex-wrap gap-x-5 gap-y-1.5 pt-4 text-sm">
                  {blocks.has(card.id) ? (
                    <a href={`#${card.id}`} className="font-medium text-fg-1 transition-colors hover:text-accent">
                      How it is built<span className="sr-only">: {card.name}</span>
                    </a>
                  ) : null}
                  <Link
                    href={`/showcase/${card.slug}/`}
                    prefetch={false}
                    className="font-medium text-fg-1 transition-colors hover:text-accent"
                  >
                    Its page<span className="sr-only">: {card.name}</span>
                    <span aria-hidden> →</span>
                  </Link>
                </p>
              </div>
            </article>
          </li>
        ))}
      </ul>
    </nav>
  );
}

/** One app: what it proves, the app running, the code of its hardest part, its numbers and its screens to try. */
function AppBlock({ app, n }: { app: Showcase; n: number }) {
  const source = `apps/${app.app}`;
  const code = excerpt(app.code);
  return (
    <article id={app.id} aria-labelledby={`${app.id}-title`} className="scroll-mt-24 border-t border-line pt-10 lg:pt-12">
      <div className="grid gap-x-12 gap-y-6 lg:grid-cols-12">
        <div className="lg:col-span-7">
          <p className="font-mono text-xs text-fg-3">
            <span className="numeric">{String(n).padStart(2, "0")}</span>
            <span aria-hidden> · </span>
            {app.name}
            <span aria-hidden> · </span>
            {app.ui}
          </p>
          <h3 id={`${app.id}-title`} className="mt-3 font-display text-display-md font-semibold text-balance text-fg">
            {app.title}
          </h3>
          <p className="mt-4 max-w-2xl text-[0.9375rem] leading-relaxed text-pretty text-fg-2 sm:text-base">
            <RichText text={app.lead} />
          </p>
        </div>
        <ul className="space-y-2.5 text-[0.9375rem] leading-relaxed text-pretty text-fg-2 lg:col-span-5 lg:pt-8">
          {app.proves.map((line) => (
            <li key={line} className="flex gap-3">
              <span aria-hidden className="mt-[0.6em] size-1.5 shrink-0 rounded-full bg-fg-3" />
              <span>
                <RichText text={line} />
              </span>
            </li>
          ))}
        </ul>
      </div>

      <figure className="mt-10 lg:mt-12">
        <div className="relative">
          <BrowserFrame address={app.screenshot.address} className="shadow-glow-sm">
            <ShowcaseImage
              name={app.screenshot.image}
              sizes="(min-width: 1440px) 1360px, (min-width: 1024px) calc(100vw - 80px), (min-width: 640px) calc(100vw - 64px), calc(100vw - 48px)"
            />
          </BrowserFrame>
          {app.screenshot.closeUp ? (
            <div className="absolute -bottom-10 left-[3.5%] hidden w-[11.5rem] lg:block xl:w-[13rem]">
              <PhoneFrame className="shadow-float">
                <ShowcaseImage name={app.screenshot.closeUp} sizes="208px" />
              </PhoneFrame>
            </div>
          ) : null}
        </div>
        <figcaption
          className={cn(
            "mt-3 text-sm leading-relaxed text-pretty text-fg-3 lg:mt-4",
            app.screenshot.closeUp && "lg:ml-[calc(3.5%+13rem)] xl:ml-[calc(3.5%+14.5rem)]",
          )}
        >
          {app.name} running on a laptop with <code className="inline-code">{app.run}</code>
          {app.screenshot.closeUp ? <span className="max-lg:hidden">, and the same drawer on a phone</span> : null}. A
          screenshot, taken on {longDate(app.numbers.date)}.
        </figcaption>
      </figure>

      <div className="mt-12 grid gap-x-12 gap-y-8 lg:mt-20 lg:grid-cols-12">
        <div className="lg:col-span-5">
          <div className="lg:sticky lg:top-[calc(var(--header-h)+2rem)]">
            <p className="eyebrow">The hard part</p>
            <p className="mt-4 text-lg leading-relaxed text-pretty text-fg-1">
              <RichText text={app.code.caption} />
            </p>
            <dl className="mt-8 grid grid-cols-2 gap-px overflow-hidden rounded-xl border border-line bg-line">
              {app.facts.map((fact) => (
                <div key={fact.label} className="bg-ink-1 px-4 py-3">
                  <dt className="text-xs text-fg-3">{fact.label}</dt>
                  <dd className="mt-1 text-[0.9375rem] font-medium text-fg-1">{fact.value}</dd>
                </div>
              ))}
            </dl>
            <p className="mt-4 text-sm leading-relaxed text-pretty text-fg-3">
              {app.numbers.text} Measured on {longDate(app.numbers.date)}; the method and every page are on{" "}
              <Link href={`/showcase/${app.slug}/#scale`} prefetch={false} className="link">
                its page
              </Link>
              .
            </p>
          </div>
        </div>
        <div className="min-w-0 lg:col-span-7">
          <CodePanel file={app.code.file} lang="rust" source={code} />
        </div>
      </div>

      <div className="mt-14 lg:mt-20">
        <h4 className="font-display text-display-sm font-semibold text-fg">Try every screen</h4>
        <p className="mt-2 max-w-2xl text-sm leading-relaxed text-pretty text-fg-3">
          Each is the app&apos;s own view, drawn by navree-ui from its seed, so you can scroll the drawer here. Links and forms
          go nowhere outside the app.
        </p>
        <Tabs defaultValue={app.views[0].example} className="mt-5 gap-4">
          <TabsList variant="line" aria-label={`${app.name}: its screens`} className="max-w-full flex-wrap justify-start border-b border-line">
            {app.views.map((view) => (
              <TabsTrigger key={view.example} value={view.example} className="flex-none">
                {view.label}
              </TabsTrigger>
            ))}
          </TabsList>
          {app.views.map((view, i) => (
            // Only the open tab's frame is in the page: the others mount when chosen.
            <TabsContent key={view.example} value={view.example}>
              <Preview id={view.example} title={view.title} caption={view.caption} interactive={view.interactive} defer={i === 0} />
            </TabsContent>
          ))}
        </Tabs>
      </div>

      <div className="mt-10 flex flex-col gap-4 border-t border-line pt-8 lg:flex-row lg:items-center lg:justify-between">
        <p className="flex flex-wrap items-center gap-x-6 gap-y-2">
          <ArrowLink href={`/showcase/${app.slug}/`}>Open the showcase page</ArrowLink>
          <ArrowLink href={repoTree(source)}>Source on GitHub</ArrowLink>
        </p>
        <CommandLine command={app.run} className="lg:min-w-[26rem]" />
      </div>
    </article>
  );
}

/** The most lines a hard part's card shows. */
const TILE_LINES = 16;

/**
 * A hard part's code: its file's lines, or its ui-docs example's code, which
 * must be in the file it names. The card wraps a line too long for it.
 */
function hardPartCode(part: HardPart): string {
  if (!part.code.example) return excerpt({ file: part.code.file, parts: part.code.parts ?? [], more: part.code.more }, TILE_LINES);
  const code = example(part.code.example).code;
  const file = fs.readFileSync(path.resolve(process.cwd(), "..", "..", part.code.file), "utf8");
  if (!file.includes(code.split("\n")[0].trim())) {
    throw new Error(`build: the example ${part.code.example} is not in ${part.code.file}`);
  }
  const lines = part.code.parts ? cut(code, part.code.parts, part.code.example, { more: part.code.more, max: TILE_LINES }) : code;
  if (lines.split("\n").length > TILE_LINES) {
    throw new Error(`build: the code of the hard part ${part.id} is longer than ${TILE_LINES} lines`);
  }
  return lines;
}

/** Every hard component on its own: a picture, what is hard, a few lines of its code and where it is used. */
function HardParts() {
  const names = new Map(showcases.map((s) => [s.id, s.name]));
  return (
    <div className="mt-20 border-t border-line pt-10 lg:mt-28 lg:pt-12">
      <div className="grid gap-x-12 gap-y-4 lg:grid-cols-12 lg:items-end">
        <h3 id="hard-parts" className="font-display text-display-md font-semibold text-balance text-fg lg:col-span-7">
          Every hard part, on its own.
        </h3>
        <p className="max-w-xl text-[0.9375rem] leading-relaxed text-pretty text-fg-2 lg:col-span-5 sm:text-base">
          The pieces that take the most work in an app, each a toolkit component with its docs page: the picture is the
          component, the code is what draws it.
        </p>
      </div>
      <ul aria-labelledby="hard-parts" className="mt-10 grid grid-cols-2 gap-3 lg:grid-cols-3">
        {hardParts.map((part) => (
          <li key={part.id} className="min-w-0">
            <article className="flex h-full flex-col overflow-hidden rounded-2xl border border-line bg-ink-1 shadow-bezel transition-colors duration-200 hover:border-line-2">
              <div className="aspect-[16/10] overflow-hidden border-b border-line bg-ink-0">
                <ShowcaseImage
                  name={part.image}
                  sizes="(min-width: 1440px) 450px, (min-width: 1024px) 31vw, 46vw"
                  className="size-full object-cover object-top-left"
                />
              </div>
              <div className="flex min-w-0 flex-1 flex-col p-3.5 sm:p-5">
                <h4 className="font-display text-[1rem] leading-snug font-semibold text-balance text-fg sm:text-display-sm">
                  <Link href={part.docs} prefetch={false} className="transition-colors hover:text-accent">
                    {part.title}
                  </Link>
                </h4>
                <p className="mt-2 text-[0.9375rem] leading-relaxed text-pretty text-fg-2 max-sm:hidden">{part.hard}</p>
                <div className="mt-4 max-sm:hidden">
                  <CodePanel file={part.code.file} lang="rust" source={hardPartCode(part)} size="sm" wrap />
                </div>
                <p className="mt-auto flex flex-wrap items-center justify-between gap-x-4 gap-y-1 pt-4 text-[0.8125rem] text-fg-3">
                  {part.seenIn ? (
                    <span>
                      Seen in{" "}
                      <a href={`#${part.seenIn}`} className="text-fg-1 transition-colors hover:text-accent">
                        {names.get(part.seenIn)}
                      </a>
                    </span>
                  ) : (
                    <span>In the toolkit</span>
                  )}
                  <Link href={part.docs} prefetch={false} className="font-medium text-fg-1 transition-colors hover:text-accent">
                    Docs<span className="sr-only">: {part.title}</span>
                    <span aria-hidden> →</span>
                  </Link>
                </p>
              </div>
            </article>
          </li>
        ))}
      </ul>
    </div>
  );
}

/**
 * "Build anything": the example apps in the repository, read from
 * `showcases.ts`. A card per app at the top; then each app drawn with Navree
 * UI as a screenshot of it running, the code of its hardest part, its numbers
 * and its screens to try; then every hard part on its own. The screenshots are
 * `scripts/showcase-images.ts`'s, the code is cut from the files when the site
 * is built, and the numbers are read from the files that measured them.
 */
export function BuildAnything() {
  return (
    <HomeSection id="build-anything">
      <SectionHeading
        split
        id="build-anything"
        eyebrow="Build anything"
        title="What you can build, and the Rust that builds it."
        lead={
          <p>
            Whole apps from the repository, screenshotted while they run, each beside the code that draws its hardest part.
            Then every hard part on its own.
          </p>
        }
        action={<ArrowLink href="/showcase/">Every example app</ArrowLink>}
      />
      <AppIndex cards={appCards} />
      <div className="mt-16 space-y-20 lg:mt-24 lg:space-y-28">
        {showcases.map((app, at) => (
          <AppBlock key={app.id} app={app} n={at + 1} />
        ))}
      </div>
      <HardParts />
      <p className="mt-14 flex flex-wrap items-center gap-x-6 gap-y-3 text-[0.9375rem] text-fg-2 lg:mt-16">
        <span>Every app drawn with Navree UI here runs on your machine with one command.</span>
        <ArrowLink href="/showcase/">Every example app</ArrowLink>
      </p>
    </HomeSection>
  );
}
