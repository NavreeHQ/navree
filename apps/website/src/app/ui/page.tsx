import type { Metadata } from "next";
import { Suspense, type ReactNode } from "react";

import { HomeSection, SectionHeading } from "@/components/home/parts";
import { TableShowcase } from "@/components/ui-docs/table-showcase";
import { pageMetadata } from "@/lib/metadata";

import { ChooseFrontEnd } from "./choose";
import { UiClosing } from "./closing";
import { BringYourDesign } from "./design";
import { UiFeatures } from "./features";
import { ComponentGallery } from "./gallery";
import { UiHero } from "./hero";
import { HowItWorks } from "./how";
import { UiRoadmap } from "./roadmap";

export const metadata: Metadata = pageMetadata({
  title: "Navree UI: your app's screens in Rust",
  description:
    "Navree UI: typed Rust components rendered to HTML on the server, with one stylesheet, a small runtime and a strict content security policy. Every component with its live preview, the front-end choices, and what comes next.",
  path: "/ui/",
});

/**
 * `/ui/`, the page about Navree UI, top to bottom: what it is, the choice of
 * front end, how it works, a live example, every component, what every page
 * can do, what comes next, bringing a design, and the way to start. The
 * components, the sizes and the roadmap are read from the files that own
 * them, so the page follows the toolkit without edits.
 */
export default function UiPage() {
  return (
    <>
      <UiHero />
      <Part>
        <ChooseFrontEnd />
      </Part>
      <Part>
        <HowItWorks />
      </Part>
      <Part>
        <HomeSection id="live">
          <SectionHeading
            split
            id="live"
            eyebrow="Real code"
            title="Real components, from real Rust."
            lead={
              <p>
                The DataTable example from the docs: its Rust, and the page navree-ui renders from it. Nothing here is a
                mock-up.
              </p>
            }
          />
          <TableShowcase className="mt-12 lg:mt-14" />
        </HomeSection>
      </Part>
      <Part>
        <ComponentGallery />
      </Part>
      <Part>
        <UiFeatures />
      </Part>
      <Part>
        <UiRoadmap />
      </Part>
      <Part>
        <BringYourDesign />
      </Part>
      <Part>
        <UiClosing />
      </Part>
    </>
  );
}

/** One section, hydrated on its own (see the homepage). */
function Part({ children }: { children: ReactNode }) {
  return <Suspense>{children}</Suspense>;
}
