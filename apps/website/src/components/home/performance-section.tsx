import { PerfHighlights } from "@/components/bench/perf-highlights";
import { ArrowLink } from "@/components/home/parts";
import { perf } from "@/lib/bench";

/**
 * The homepage's performance section: four of Navree's own numbers, one
 * sentence on how they were measured and a link to /performance/.
 */
export function PerformanceSection() {
  const checks = perf.conformance.passed;
  return (
    <section aria-labelledby="home-performance-heading">
      <div className="container-site section-y">
        <div className="grid gap-x-12 gap-y-6 lg:grid-cols-12 lg:items-end">
          <div className="lg:col-span-7">
            <p className="eyebrow">Performance</p>
            <h2
              id="home-performance-heading"
              className="mt-5 max-w-2xl text-display-lg font-semibold text-balance text-fg"
            >
              Fast and frugal, measured on a real API.
            </h2>
          </div>
          <div className="lg:col-span-5">
            <p className="max-w-xl text-lg leading-relaxed text-pretty text-fg-2">
              One {perf.label} binary on {perf.settings.app.count} CPUs, serving a CRM-style API with JWT auth, validation
              and pagination against PostgreSQL{checks ? `, after passing a ${checks}-check conformance test` : ""}.
            </p>
            <ArrowLink href="/performance/" className="mt-5">
              How it&apos;s measured
            </ArrowLink>
          </div>
        </div>
        <PerfHighlights className="mt-12 lg:mt-16" />
      </div>
    </section>
  );
}
