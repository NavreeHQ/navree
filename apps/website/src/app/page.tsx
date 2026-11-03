import type { Metadata } from "next";
import { Suspense, type ReactNode } from "react";

import { AiSection } from "@/components/home/ai-section";
import { ArchitectureSection } from "@/components/home/architecture-section";
import { CrmSection } from "@/components/home/crm-section";
import { EcosystemSection } from "@/components/home/ecosystem-section";
import { FinalCta } from "@/components/home/final-cta";
import { Hero } from "@/components/home/hero";
import { InTheBox } from "@/components/home/in-the-box";
import { PerformanceSection } from "@/components/home/performance-section";
import { Pillars } from "@/components/home/pillars";
import { Ship } from "@/components/home/ship";
import { Showcase } from "@/components/home/showcase";
import { TwoWays } from "@/components/home/two-ways";
import { UiSection } from "@/components/home/ui-section";
import { WhatIs } from "@/components/home/what-is";
import { Workspaces } from "@/components/home/workspaces";
import { pageMetadata } from "@/lib/metadata";
import { PageSchema } from "@/lib/structured-data";
import { site } from "@site";

export const metadata: Metadata = pageMetadata({
  title: `${site.name}: ${site.headline}`,
  absoluteTitle: true,
  description: site.description,
  path: "/",
});

/**
 * The homepage, top to bottom: what Navree is, what every app gets, Navree
 * UI and the other ways to draw screens, the code, the reference CRM built
 * with it, the shapes of system it builds, workspaces, AI, the two ways in
 * from an existing product or design, the ecosystem, shipping, performance,
 * the crates, and the way to start. The sections that
 * summarize another page (architecture, AI, ecosystem, performance) render
 * their own content; each sits under a hairline divider drawn here.
 *
 * Every section below the hero is its own Suspense boundary. Nothing
 * suspends; the boundaries let React hydrate the page a section at a time,
 * in short tasks, instead of in one long one.
 */
export default function Home() {
  return (
    <>
      <Hero />
      <Part>
        <WhatIs />
      </Part>
      <Part>
        <Pillars />
      </Part>
      <Part>
        <UiSection />
      </Part>
      <Part>
        <Showcase />
      </Part>
      <Part>
        <CrmSection />
      </Part>
      <Part divided>
        <ArchitectureSection />
      </Part>
      <Part>
        <Workspaces />
      </Part>
      <Part divided>
        <AiSection />
      </Part>
      <Part>
        <TwoWays />
      </Part>
      <Part divided>
        <EcosystemSection />
      </Part>
      <Part>
        <Ship />
      </Part>
      <Part divided>
        <PerformanceSection />
      </Part>
      <Part>
        <InTheBox />
      </Part>
      <Part>
        <FinalCta />
      </Part>
      <PageSchema path="/" metadata={metadata} />
    </>
  );
}

/**
 * One section, hydrated on its own. `divided` draws the hairline above the
 * sections that summarize another page.
 */
function Part({ children, divided }: { children: ReactNode; divided?: boolean }) {
  return <Suspense>{divided ? <div className="cv-auto border-t border-line">{children}</div> : children}</Suspense>;
}
