import type { Metadata } from "next";
import type { ReactNode } from "react";

import { Faq } from "@/components/faq";
import { HomeSection, SectionHeading } from "@/components/home/parts";
import { TableShowcase } from "@/components/ui-docs/table-showcase";
import { pageMetadata } from "@/lib/metadata";
import { PageSchema } from "@/lib/structured-data";

import { BuildAnything } from "./build";
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
    "Typed Rust components rendered to HTML on the server, with one stylesheet and a strict content security policy. A live preview of every component.",
  path: "/ui/",
});

/**
 * `/ui/`, the page about Navree UI, top to bottom: what it is, the choice of
 * front end, how it works, a live example, every component, an example app
 * built from them, what every page can do, what comes next, bringing a design, and the way to start. The
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
        <BuildAnything />
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
      <Faq name="ui" />
      <PageSchema path="/ui/" metadata={metadata} />
    </>
  );
}

/**
 * One section. Not a Suspense boundary: the async sections would be written as
 * hidden blocks swapped in by a script, invisible with JavaScript off and
 * loading every preview iframe at once (see the homepage).
 */
function Part({ children }: { children: ReactNode }) {
  return children;
}
