import { CrmBlock } from "./crm-section";
import { ExampleAppsBlock } from "./example-apps";
import { HomeSection, SectionHeading } from "./parts";

/**
 * "Built with Navree": the reference CRM, whose screens are drawn by Next.js
 * and Nuxt, and the example apps whose screens Navree UI draws, under one
 * heading. The two used to be sections of their own.
 */
export function AppsSection() {
  return (
    <HomeSection id="apps">
      <SectionHeading
        split
        id="apps"
        eyebrow="Built with Navree"
        title="Whole apps, not snippets."
        lead={
          <p>
            A reference CRM with two front ends, and example apps drawn with Navree UI. Each is in the repository, runs on
            your machine and is tested with the framework.
          </p>
        }
      />
      <div className="mt-14 space-y-20 lg:mt-20 lg:space-y-28">
        <CrmBlock />
        <ExampleAppsBlock />
      </div>
    </HomeSection>
  );
}
