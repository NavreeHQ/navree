import { groups } from "@/components/ui-docs/data";
import { aiFaq, homeFaq, introductionFaq, uiFaq, type FaqSet } from "@/content/faq";
import { uiRoadmap } from "@/lib/ui-page-data";

export type FaqName = "home" | "ui" | "ai" | "introduction";

/**
 * Every set of questions, by name, for the pages that draw them. The Navree
 * UI set is computed from the catalogue and the roadmap, which is why this
 * file is separate from `faq.ts`: that one has no site aliases and is also
 * read by the script that writes the Markdown copies.
 */
export function faqSets(): Record<FaqName, FaqSet> {
  return { home: homeFaq, ui: uiFaq({ groups, roadmap: uiRoadmap() }), ai: aiFaq, introduction: introductionFaq };
}
