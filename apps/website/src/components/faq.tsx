import Link from "next/link";

import { ArrowLink, HomeSection, SectionHeading } from "@/components/home/parts";
import { InlineMarkdown } from "@/components/inline-markdown";
import { docFaq, plainAnswer, type FaqItem, type FaqSet } from "@/content/faq";
import { faqSets, type FaqName } from "@/content/faq-sets";
import { JsonLd, faqData } from "@/lib/structured-data";

/**
 * A block of questions and answers, drawn from one data set
 * (`src/content/faq.ts`) that also writes the page's `FAQPage` structured
 * data, so what a reader sees and what a crawler reads are the same words.
 *
 *     <Faq name="home" />        a section of a marketing page
 *     <DocFaq slug={doc.slug} /> the end of a docs page that has questions
 *
 * Questions are `<h3>` headings and answers stay visible (no accordion), so
 * they can be found, linked and quoted.
 */

function Answer({ item }: { item: FaqItem }) {
  return (
    <>
      <p className="mt-3 max-w-2xl text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
        <InlineMarkdown text={item.answer} />
      </p>
      {item.link ? (
        <ArrowLink href={item.link.href} className="mt-4">
          {item.link.label}
        </ArrowLink>
      ) : null}
    </>
  );
}

function Schema({ set }: { set: FaqSet }) {
  return (
    <JsonLd
      data={faqData(
        set.path,
        set.items.map((item) => ({ question: item.question, answer: plainAnswer(item.answer) })),
      )}
    />
  );
}

/** A section of a marketing page: a hairline, the container and the section rhythm. */
export function Faq({ name }: { name: FaqName }) {
  const set = faqSets()[name];
  return (
    <HomeSection id="faq">
      <SectionHeading split id="faq" eyebrow="Questions" title={set.heading} />
      <div className="mt-10 lg:mt-14">
        {set.items.map((item) => (
          <div key={item.question} className="border-t border-line py-7 first:border-t-0 first:pt-0 lg:grid lg:grid-cols-12 lg:gap-12">
            <h3 className="text-lg font-semibold text-balance text-fg lg:col-span-5">{item.question}</h3>
            <div className="mt-1 lg:col-span-7 lg:mt-0">
              <Answer item={item} />
            </div>
          </div>
        ))}
      </div>
      <Schema set={set} />
    </HomeSection>
  );
}

/** The questions of a docs page, in the page's own typography. Nothing when the page has none. */
export function DocFaq({ slug }: { slug: string }) {
  const set = docFaq(slug);
  if (!set) return null;
  return (
    <section aria-labelledby="common-questions" className="prose-navree mt-14">
      <h2 id="common-questions">{set.heading}</h2>
      {set.items.map((item) => (
        <div key={item.question}>
          <h3>{item.question}</h3>
          <p>
            <InlineMarkdown text={item.answer} />
          </p>
          {item.link ? (
            <p>
              <Link href={item.link.href}>{item.link.label}</Link>
            </p>
          ) : null}
        </div>
      ))}
      <Schema set={set} />
    </section>
  );
}
