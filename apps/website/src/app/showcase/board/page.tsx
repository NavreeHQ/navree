import { ChevronRightIcon } from "lucide-react";
import type { Metadata } from "next";
import Link from "next/link";
import type { ReactNode } from "react";

import { PrimaryCta, SecondaryCta, SectionHeading } from "@/components/home/parts";
import { boardNumbers } from "@/components/showcase/board-numbers";
import { CodePanel, CommandLine } from "@/components/showcase/code-panel";
import { example } from "@/components/ui-docs/data";
import { PreviewFrame } from "@/components/ui-docs/preview-frame";
import { indian } from "@/lib/format";
import { pageMetadata } from "@/lib/metadata";
import { PageSchema } from "@/lib/structured-data";
import { cn } from "@/lib/utils";
import { repoTree } from "@site";

const APP = "apps/board-demo";

export const metadata: Metadata = pageMetadata({
  title: "A team's board, built on Navree",
  description: `A product board of ${indian(boardNumbers.cards)} cards: drag, touch and keyboard moves checked by the server, live for every viewer over server-sent events, columns loaded a page at a time.`,
  path: "/showcase/board/",
});

/** A section of this page: a hairline on top, the container and the rhythm. */
function Section({ id, children, className }: { id: string; children: ReactNode; className?: string }) {
  return (
    <section id={id} aria-labelledby={`${id}-heading`} className={cn("cv-auto scroll-mt-(--header-h) border-t border-line", className)}>
      <div className="container-site section-y">{children}</div>
    </section>
  );
}

/** One screen: the app's own view, rendered by navree-ui, with a caption. */
function Screen({ id, title, caption }: { id: string; title: string; caption: ReactNode }) {
  const preview = example(id);
  return (
    <figure className="bezel min-w-0">
      <div className="bezel-core overflow-hidden" inert>
        <PreviewFrame src={preview.src} title={title} height={preview.height} width={preview.width} />
      </div>
      <figcaption className="px-3 pt-3 pb-1.5 text-sm leading-relaxed text-pretty text-fg-3">{caption}</figcaption>
    </figure>
  );
}

const kb = (bytes: number) => `${Math.round(bytes / 1024)} KB`;

/** The figures measured on the running app, from `board-numbers.ts`. */
const numbers = [
  {
    value: indian(boardNumbers.cards),
    label: "cards",
    text: `with ${indian(boardNumbers.comments)} comments and ${indian(boardNumbers.history)} lines of history, written at start in about ${boardNumbers.seedSeconds} s`,
  },
  {
    value: `${boardNumbers.firstPage.brotliKb} KB`,
    label: "first page, brotli",
    text: `${kb(boardNumbers.firstPage.htmlBytes)} of HTML: the first ${boardNumbers.page} cards of each column`,
  },
  {
    value: `${(boardNumbers.sortableBrotli / 1024).toFixed(1)} KB`,
    label: "sortable module, brotli",
    text: "loaded only by pages that have a sortable",
  },
  {
    value: indian(boardNumbers.nodes),
    label: "DOM nodes",
    text: `after scrolling two columns of thousands to their cap of ${boardNumbers.cap} cards each`,
  },
];

const plans = [
  ["A column's first page", "SEARCH cards USING INDEX cards_place (list=?)"],
  ["The next page, by cursor", "SEARCH cards USING INDEX cards_place (list=? AND rank>?)"],
  ["A column's count", "SEARCH cards USING COVERING INDEX cards_assignee (list=?)"],
  ["A move's neighbour", "SEARCH cards USING INDEX cards_place (list=? AND rank<?)"],
  ["A card's newest comments", "SEARCH comments USING INDEX comments_card (card_id=?)"],
];

export default function BoardShowcasePage() {
  const columns = example("build-anything/board-columns");
  const card = example("build-anything/board-card");
  return (
    <>
      <section aria-labelledby="board-heading" className="relative isolate overflow-hidden">
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
                Board
              </li>
            </ol>
          </nav>
          <div className="grid gap-x-12 gap-y-8 lg:grid-cols-12 lg:items-end">
            <div className="lg:col-span-7">
              <p className="eyebrow">Example app</p>
              <h1 id="board-heading" className="mt-5 text-display-xl font-semibold text-balance text-fg">
                <span className="block">A team&apos;s board,</span>
                <span className="block text-fg-2">built on Navree.</span>
              </h1>
            </div>
            <div className="lg:col-span-5 lg:pb-2">
              <p className="max-w-xl text-lg leading-relaxed text-pretty text-fg-2">
                A product team plans a release on five columns of {indian(boardNumbers.cards)} cards. You move a card with a mouse, a finger or the
                keys and it lands at once; your server checks the move against the board as it is now, and every other open board
                slides the card into place. It is one Rust crate on SQLite, in the repository.
              </p>
              <div className="mt-8 flex flex-wrap items-center gap-3">
                <PrimaryCta href="#run">Run it locally</PrimaryCta>
                <SecondaryCta href="/docs/ui/drag-and-drop/">Read the guide</SecondaryCta>
              </div>
            </div>
          </div>
          <div className="mt-12 pb-16 lg:mt-16">
            <Screen
              id={columns.id}
              title="A product team's board: five columns of cards with labels, due dates, assignees and limits"
              caption="The board, drawn by navree-ui from the app's seed. Replayed here: a teammate's two moves slide in. In the app, the server streams them."
            />
          </div>
        </div>
      </section>

      <Section id="screens">
        <SectionHeading
          split
          id="screens-heading"
          eyebrow="The screens"
          title="A move from start to finish."
          lead={<p>Each picture is the app&apos;s own Rust, rendered ahead of time from the same seed the app writes at start.</p>}
        />
        <div className="mt-12 grid gap-6 lg:mt-16">
          <Screen
            id="build-anything/board-drag"
            title="A card picked up with the keys and carried into Review, the board saying where it is"
            caption="Part way through a move. With the keys, each step is said: “Position 2 of 2 in Review.” With a mouse or a finger, a copy follows you and the cards make room."
          />
          <Screen
            id={card.id}
            title="A card's details: description, checklist, comments and history"
            caption="A card as its drawer shows it over the board, at about 60% of the width. Older comments and history load as you scroll."
          />
          <Screen
            id="build-anything/board-sync"
            title="Two people's screens of one board, a card arriving on the second"
            caption="Two viewers. Gurpreet moves a card to Review; on Harleen's screen it slides in, replayed here. In the app, a server-sent event tells her board to fetch itself again."
          />
        </div>
      </Section>

      <Section id="built">
        <SectionHeading
          split
          id="built-heading"
          eyebrow="How it is built"
          title="The page moves the card. Your server decides."
          lead={
            <p>
              The board is a <Link href="/docs/components/sortable/" className="link">Sortable</Link> in a{" "}
              <Link href="/docs/ui/regions/" className="link">live region</Link>. A drop posts a <code className="inline-code">Move</code>;
              the handler checks it in the transaction that writes it, and refuses a stale move, a full column or a card a member may not close.
            </p>
          }
        />
        <div className="mt-12 grid gap-4 lg:grid-cols-2">
          <CodePanel file={`${APP}/src/views/board.rs`} lang="rust" source={columns.code} />
          <CodePanel file={`${APP}/src/views/card.rs`} lang="rust" source={card.code} />
        </div>
      </Section>

      <Section id="scale">
        <SectionHeading
          split
          id="scale-heading"
          eyebrow="At scale"
          title="Twenty thousand cards, read a page at a time."
          lead={
            <p>
              Each column draws its first {boardNumbers.page} cards and loads more as it is scrolled, by a cursor on an index, up to {boardNumbers.cap} in the page. A
              drop below the last card drawn lands right after it, wherever that is in the column. Measured on a laptop, a debug build,
              over loopback.
            </p>
          }
        />
        <dl className="mt-12 grid gap-px overflow-hidden rounded-xl border border-line bg-line sm:grid-cols-2 lg:grid-cols-4">
          {numbers.map((n) => (
            <div key={n.label} className="bg-ink-0 p-6">
              <dt className="text-sm text-fg-3">{n.label}</dt>
              <dd className="mt-2 font-display text-display-sm font-semibold text-fg numeric">{n.value}</dd>
              <dd className="mt-2 text-sm leading-relaxed text-pretty text-fg-2">{n.text}</dd>
            </div>
          ))}
        </dl>
        <h3 className="mt-12 font-display text-lg font-semibold text-fg">Every query is an index search</h3>
        <ul className="mt-4 divide-y divide-line border-y border-line">
          {plans.map(([what, plan]) => (
            <li key={what} className="grid gap-1 py-2.5 text-sm sm:grid-cols-[14rem_1fr]">
              <span className="text-fg-2">{what}</span>
              <code className="font-mono text-[0.8125rem] break-words text-fg-1">{plan}</code>
            </li>
          ))}
        </ul>
      </Section>

      <Section id="run">
        <SectionHeading
          split
          id="run-heading"
          eyebrow="Run it"
          title="On your machine in one command."
          lead={
            <p>
              The app writes its board afresh at every start. A teammate moves a card on every 25 seconds or so; turn that off with{" "}
              <code className="inline-code">BOARD_SIMULATOR=off</code>.
            </p>
          }
        />
        <div className="mt-10 grid max-w-3xl gap-3">
          <CommandLine command="cargo run -p board-demo" comment="http://127.0.0.1:8097" />
          <p className="text-[0.9375rem] leading-relaxed text-fg-2">
            Open it in two windows and move a card in one.{" "}
            <a href={repoTree(APP)} className="link">
              Read the app&apos;s source on GitHub
            </a>
            .
          </p>
        </div>
      </Section>
      <PageSchema path="/showcase/board/" metadata={metadata} />
    </>
  );
}
