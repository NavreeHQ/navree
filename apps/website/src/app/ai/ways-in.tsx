import { WaysTabs } from "@/components/home/two-ways";

import { ArrowLink, BlockHeading } from "./parts";

/**
 * The two ways to bring what you already have into Navree: a product you run
 * (`navree-reverse-engineering`) or a design (`navree-design-to-ui`). The tabs
 * and their phase diagrams are the homepage's, so the two pages show the same
 * picture of the same skills.
 */
export function WaysIn() {
  return (
    <div>
      <div className="flex flex-wrap items-end justify-between gap-x-8 gap-y-4">
        <div className="max-w-2xl">
          <BlockHeading label="navree skills:install" title="Start from what you already have" />
          <p className="mt-5 leading-relaxed text-pretty text-fg-2">
            Two skills bring an existing product or design into Navree. Your coding agent reads, writes and builds; you decide
            at fixed stop points, and every guess comes back to you as a question.
          </p>
        </div>
        <ArrowLink href="/docs/getting-started/bring-your-product/">Bring your product</ArrowLink>
      </div>
      <WaysTabs headingLevel={4} className="mt-10" />
    </div>
  );
}
