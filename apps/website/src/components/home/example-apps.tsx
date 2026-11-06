import { showcases } from "@/app/ui/showcases";
import { ExampleAppCard } from "@/components/showcase/example-app-card";

import { ArrowLink } from "./parts";

/**
 * "More apps, drawn with Navree UI": the apps in the repository that Navree UI
 * draws, one card each, read from `src/app/ui/showcases.ts` like `/showcase/`
 * and `/ui/`, so an app added there shows up here. They follow the CRM, whose
 * screens are drawn by two other front ends. A block of the homepage's apps
 * section (`apps-section.tsx`), under its heading.
 */
export function ExampleAppsBlock() {
  return (
    <div>
      <div className="grid gap-6 lg:grid-cols-12 lg:items-end lg:gap-12">
        <h3 className="font-display text-display-md font-semibold text-balance text-fg lg:col-span-7">More apps, drawn with Navree UI.</h3>
        <div className="lg:col-span-5 lg:pb-1">
          <p className="max-w-xl text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
            Each is a whole app on SQLite with seeded data and live updates, in the repository: screens drawn by Rust on the
            server, and the code of its hardest part beside them. Every one runs on your machine with one command.
          </p>
          <ArrowLink href="/showcase/" className="mt-4">
            Every example app
          </ArrowLink>
        </div>
      </div>
      <ul className="mt-10 grid grid-cols-1 gap-3 lg:mt-12 xl:grid-cols-3">
        {showcases.map((app) => (
          <li key={app.slug} className="min-w-0">
            <ExampleAppCard app={app} headingLevel={4} />
          </li>
        ))}
      </ul>
    </div>
  );
}
