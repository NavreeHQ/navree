import { groups } from "@/components/ui-docs/data";
import { aboutFaq, aiFaq, architectureFaq, homeFaq, introductionFaq, uiFaq, type FaqSet } from "@/content/faq";
import { uiRoadmap } from "@/lib/ui-page-data";
import { appCards } from "@/app/ui/showcases";

export type FaqName = "home" | "ui" | "ai" | "introduction" | "about" | "architecture";

/**
 * Every set of questions, by name, for the pages that draw them. The Navree
 * UI set is computed from the catalogue, the roadmap and the example apps,
 * which is why this file is separate from `faq.ts`: that one has no site aliases and is also
 * read by the script that writes the Markdown copies.
 */
export function faqSets(): Record<FaqName, FaqSet> {
  return {
    home: homeFaq,
    ui: uiFaq({ groups, roadmap: uiRoadmap(), apps: appCards }),
    ai: aiFaq,
    introduction: introductionFaq,
    about: aboutFaq,
    architecture: architectureFaq,
  };
}
