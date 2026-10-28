import type { Metadata } from "next";
import type { ReactNode } from "react";

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
import { WhatIs } from "@/components/home/what-is";
import { Workspaces } from "@/components/home/workspaces";
import { pageMetadata } from "@/lib/metadata";
import { JsonLd, softwareData } from "@/lib/structured-data";
import { site } from "@site";

export const metadata: Metadata = pageMetadata({
  title: `${site.name}: ${site.headline}`,
  absoluteTitle: true,
  description: site.description,
  path: "/",
});

/**
 * The homepage, top to bottom: what Navree is, what every app gets, the
 * code, the reference CRM built with it, the shapes of system it builds, workspaces, AI, the ecosystem,
 * shipping, performance, the crates, and the way in. Sections owned by
 * other pages' workers (architecture, AI, ecosystem, performance) render
 * their own content; each sits under a hairline divider drawn here.
 */
export default function Home() {
  return (
    <>
      <Hero />
      <WhatIs />
      <Pillars />
      <Showcase />
      <CrmSection />
      <Divided>
        <ArchitectureSection />
      </Divided>
      <Workspaces />
      <Divided>
        <AiSection />
      </Divided>
      <Divided>
        <EcosystemSection />
      </Divided>
      <Ship />
      <Divided>
        <PerformanceSection />
      </Divided>
      <InTheBox />
      <FinalCta />
      <JsonLd data={softwareData()} />
    </>
  );
}

/** The hairline between sections, for the sections other workers own. */
function Divided({ children }: { children: ReactNode }) {
  return <div className="cv-auto border-t border-line">{children}</div>;
}
