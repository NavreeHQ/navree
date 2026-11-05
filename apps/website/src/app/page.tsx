import type { Metadata } from "next";
import type { ReactNode } from "react";

import { Faq } from "@/components/faq";
import { AiSection } from "@/components/home/ai-section";
import { ArchitectureSection } from "@/components/home/architecture-section";
import { CrmSection } from "@/components/home/crm-section";
import { EcosystemSection } from "@/components/home/ecosystem-section";
import { ExampleApps } from "@/components/home/example-apps";
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
import { film, lengthIso } from "@/content/launch-film";
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
 * with it, the example apps drawn with Navree UI, the shapes of system it
 * builds, workspaces, AI, the two ways in from an existing product or design,
 * the ecosystem, shipping, performance, the crates, and the way to start. The sections that
 * summarize another page (architecture, AI, ecosystem, performance) render
 * their own content; each sits under a hairline divider drawn here.
 *
 * No section sits in a Suspense boundary: several are async (their code is
 * highlighted at build time), and a boundary around an async section is
 * written as a hidden block that a script swaps in. With JavaScript off the
 * page would show only its hero, and iframes in a hidden block load at once,
 * whatever their `loading`.
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
      <Part>
        <ExampleApps />
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
      <Part>
        <Faq name="home" />
      </Part>
      <PageSchema
        path="/"
        metadata={metadata}
        video={{
          id: "film",
          name: film.name,
          description: film.description,
          thumbnail: film.poster.jpg,
          file: film.file.src,
          encodingFormat: film.file.type,
          uploadDate: film.uploadDate,
          duration: lengthIso(film.durationSeconds),
          width: film.width,
          height: film.height,
        }}
      />
    </>
  );
}

/** One section. `divided` draws the hairline above the sections that summarize another page. */
function Part({ children, divided }: { children: ReactNode; divided?: boolean }) {
  return divided ? <div className="cv-auto border-t border-line">{children}</div> : children;
}
