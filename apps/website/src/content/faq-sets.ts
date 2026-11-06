import { groups } from "@/components/ui-docs/data";
import { aboutFaq, aiFaq, architectureFaq, ecosystemFaq, homeFaq, introductionFaq, performanceFaq, uiFaq, type FaqSet } from "@/content/faq";
import { ecosystem } from "@/content/ecosystem";
import { measuredOn, perf } from "@/lib/bench";
import { screenTables } from "@/lib/screens";
import { uiRoadmap } from "@/lib/ui-page-data";
import { appCards } from "@/app/ui/showcases";

export type FaqName = "home" | "ui" | "ai" | "introduction" | "about" | "architecture" | "performance" | "ecosystem";

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
    performance: performanceFaq(performanceInput()),
    ecosystem: ecosystemFaq({ total: ecosystem.length }),
  };
}

/** The performance questions' numbers, read from the result file and the screens the example apps measured. */
function performanceInput() {
  const orders = screenTables()[0].screens.find((screen) => screen.name === "Orders");
  if (!orders) throw new Error("faq-sets: the store has no Orders screen in its measurements");
  return {
    listRps: perf.results.deals_list.rps,
    createRps: perf.results.deals_create.rps,
    cpus: perf.settings.app.count,
    peakMemMb: Math.round(perf.peakMemMb),
    startupMs: perf.startupMs,
    runs: perf.settings.runs,
    durationS: perf.settings.durationS,
    warmupS: perf.settings.warmupS,
    connections: perf.settings.connections,
    checks: perf.conformance.passed,
    measured: measuredOn(),
    store: { orders: orders.behind.replace(" orders", ""), paintMs: orders.desktopMs },
  };
}
