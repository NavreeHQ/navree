import Link from "next/link";

import { DiagramFigure } from "@/components/diagrams";
import { requestToPixels } from "@/components/diagrams/request-to-pixels";
import { ArrowLink } from "@/components/home/parts";
import { InlineMarkdown } from "@/components/inline-markdown";
import { cn } from "@/lib/utils";
import { repoBlob } from "@site";

import { rules, type Rule } from "./rules";

const number = (i: number) => String(i + 1).padStart(2, "0");

/**
 * Proof text with its `code` spans. Test and symbol names are long, so a
 * phone may break them after an underscore or `::` rather than mid-word.
 */
function ProofText({ text }: { text: string }) {
  return text.split(/(`[^`]+`)/g).map((part, i) => {
    if (!(part.startsWith("`") && part.endsWith("`") && part.length > 2)) return part;
    const pieces = part.slice(1, -1).split(/(?<=_|::)/);
    return (
      <code key={i} className="inline-code">
        {pieces.map((piece, j) => (
          <span key={j}>
            {piece}
            {j < pieces.length - 1 ? <wbr /> : null}
          </span>
        ))}
      </code>
    );
  });
}

/** One rule: its heading and two sentences, then its proof, each piece linked to its file, then where to read more. */
function RuleCard({ rule, index, wide }: { rule: Rule; index: number; wide: boolean }) {
  return (
    <li
      id={rule.id}
      className={cn(
        "flex min-w-0 scroll-mt-[calc(var(--header-h)+4.25rem)] flex-col rounded-[1.25rem] border border-line bg-ink-1/60 p-1.5",
        wide && "md:col-span-2",
      )}
    >
      <div className="px-4 pt-5 pb-6 sm:px-5">
        <p className="font-mono text-xs text-fg-3 tabular-nums">{number(index)}</p>
        <h3 className="mt-3 font-display text-display-sm font-semibold text-balance text-fg">{rule.title}</h3>
        <p className="mt-3 max-w-2xl text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
          <InlineMarkdown text={rule.text} />
        </p>
      </div>
      {/* The proof grows to the card's height, so the proofs of a row start on one line. */}
      <div className="flex-1 rounded-[0.875rem] border border-line bg-ink-0/60 px-4 py-4 sm:px-5">
        <p className="font-mono text-xs tracking-[0.12em] text-fg-3 uppercase">Proof</p>
        <ul className="mt-3 space-y-3">
          {rule.proof.map((proof) => (
            <li key={proof.what} className="text-[0.8125rem] leading-relaxed text-pretty text-fg-1 [&_code]:[overflow-wrap:anywhere]">
              <ProofText text={proof.what} />
              <span className="mt-0.5 flex flex-wrap items-baseline gap-x-2 font-mono text-xs text-fg-3">
                <a
                  href={repoBlob(proof.file)}
                  className="[overflow-wrap:anywhere] underline decoration-line-2 underline-offset-[3px] transition-colors hover:text-fg-1 hover:decoration-fg-3"
                >
                  {proof.file}
                </a>
                {proof.note ? <span>{proof.note}</span> : null}
              </span>
            </li>
          ))}
        </ul>
      </div>
      <div className="flex flex-wrap items-center gap-x-6 gap-y-2 px-4 pt-4 pb-3 sm:px-5">
        <ArrowLink href={rule.docs.href} className="text-sm">
          <span>
            <span className="text-fg-3">Docs: </span>
            {rule.docs.label}
          </span>
        </ArrowLink>
        <Link href={rule.story.href} className="text-sm text-fg-3 transition-colors hover:text-fg-1">
          {rule.story.label}
        </Link>
      </div>
    </li>
  );
}

/**
 * "Built in": the rules every Navree app follows, with the picture of a page
 * from the request to the pixels. Every rule's proof is in the repository
 * (`./rules.ts` fails the build when one is not), so every rule is drawn.
 */
export function BuiltIn() {
  const shown = rules();
  if (shown.length === 0) return null;
  return (
    <section id="built-in" aria-labelledby="built-in-heading" className="cv-auto scroll-mt-[calc(var(--header-h)+3.25rem)] border-b border-line">
      <div className="container-site section-y">
        <div className="grid grid-cols-1 items-center gap-y-10 xl:grid-cols-12 xl:gap-x-16">
          <div className="xl:col-span-5">
            <p className="eyebrow">Built in</p>
            <h2 id="built-in-heading" className="mt-5 text-display-lg font-semibold text-balance text-fg">
              Rules every Navree app follows, whatever its shape.
            </h2>
            <p className="mt-6 max-w-2xl text-lg leading-relaxed text-pretty text-fg-2">
              Each one is in the code, with a test or a measurement you can read and a page of the docs that explains it. The
              rules for pages hold for screens drawn with Navree UI; the picture follows one from the request to the pixels,
              and on through a live update.
            </p>
            <ArrowLink href="/about/#lessons" className="mt-6">
              The lessons behind them
            </ArrowLink>
          </div>
          <div className="min-w-0 xl:col-span-7">
            <DiagramFigure spec={requestToPixels} name="request-to-pixels" />
          </div>
        </div>

        <ol className="mt-14 grid grid-cols-1 gap-3 md:grid-cols-2 lg:mt-16">
          {shown.map((rule, i) => (
            <RuleCard key={rule.id} rule={rule} index={i} wide={shown.length % 2 === 1 && i === shown.length - 1} />
          ))}
        </ol>
      </div>
    </section>
  );
}
