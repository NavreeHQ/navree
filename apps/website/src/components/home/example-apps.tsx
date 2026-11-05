import { showcases } from "@/app/ui/showcases";
import { ExampleAppCard } from "@/components/showcase/example-app-card";

import { ArrowLink, HomeSection, SectionHeading } from "./parts";

/**
 * "Example apps": the apps in the repository that Navree UI draws, one card
 * each, read from `src/app/ui/showcases.ts` like `/showcase/` and `/ui/`, so
 * an app added there shows up here. They follow the CRM, which has the
 * section above to itself because its screens are drawn by two other front
 * ends.
 */
export function ExampleApps() {
  return (
    <HomeSection id="example-apps">
      <SectionHeading
        split
        id="example-apps"
        eyebrow="Example apps"
        title="More apps, drawn with Navree UI."
        lead={
          <p>
            Each is a whole app on SQLite with seeded data and live updates, in the repository: screens drawn by Rust on the
            server, and the code of its hardest part beside them. Every one runs on your machine with one command.
          </p>
        }
        action={<ArrowLink href="/showcase/">Every example app</ArrowLink>}
      />
      <ul className="mt-12 grid grid-cols-1 gap-3 lg:mt-14 xl:grid-cols-3">
        {showcases.map((app) => (
          <li key={app.slug} className="min-w-0">
            <ExampleAppCard app={app} />
          </li>
        ))}
      </ul>
    </HomeSection>
  );
}
