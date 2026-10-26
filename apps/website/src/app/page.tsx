import Link from "next/link";
import type { ReactNode } from "react";

import crates from "@/generated/crates.json";
import { BenchTeaser } from "@/components/home/bench-teaser";
import { Features } from "@/components/home/features";
import { Hero } from "@/components/home/hero";
import { InTheBox } from "@/components/home/in-the-box";
import { Showcase } from "@/components/home/showcase";
import { bench, highlighted } from "@/lib/bench";
import { site } from "@site";

function Section({
  id,
  title,
  lead,
  children,
  className = "",
}: {
  id: string;
  title: string;
  lead: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section aria-labelledby={id} className={`border-b border-line ${className}`}>
      <div className="mx-auto max-w-[90rem] px-4 py-20 sm:px-6 lg:px-8 lg:py-28">
        <h2 id={id} className="max-w-[40rem] text-[2rem] leading-[1.1] font-semibold tracking-[-0.035em] text-balance text-fg sm:text-[2.5rem]">
          {title}
        </h2>
        <div className="mt-4 max-w-[40rem] text-lg leading-relaxed text-pretty text-fg-2">{lead}</div>
        <div className="mt-12 lg:mt-14">{children}</div>
      </div>
    </section>
  );
}

export default function Home() {
  const navree = highlighted();
  const others = bench.frameworks.length - 1;
  const checks = navree.conformance.passed;
  const layers = new Set(crates.map((c) => c.layer)).size;

  return (
    <>
      <Hero />

      <Section
        id="code"
        title="Structured like NestJS, checked by Rust"
        lead={
          <p>
            Modules, providers, controllers and guards work the way you know from NestJS. The difference is underneath: one
            binary, no garbage collector, and types the compiler checks.
          </p>
        }
      >
        <Showcase />
      </Section>

      <Section
        id="features"
        title="Everything a backend needs"
        lead={
          <p>
            Like Laravel, {site.name} ships the whole stack: auth, an ORM, queues, mail, real time, testing and a CLI,
            designed to work together and documented in one place.
          </p>
        }
      >
        <Features />
      </Section>

      <Section
        id="benchmarks"
        title={`Measured against ${others} other frameworks`}
        lead={
          <p>
            The same API on the same machine, after the same {checks ? `${checks}-check ` : ""}conformance test. {site.name}{" "}
            lands mid-field where requests do real work and stays tiny in memory. Its own per-request overhead is high, and the
            benchmarks page says why.
          </p>
        }
      >
        <BenchTeaser />
      </Section>

      <Section
        id="in-the-box"
        title="Everything in the box"
        lead={
          <p>
            {crates.length} crates in {layers} layers. A crate’s dependencies all sit in lower layers, and a test checks every
            edge. Depend on the <code className="inline-code">navree</code> facade to get every module, or turn its features
            off to compile less.
          </p>
        }
      >
        <InTheBox />
        <div className="mt-14 flex flex-wrap items-center gap-3">
          <Link href="/docs/getting-started/installation/" className="btn btn-primary">
            Get started
          </Link>
          <Link href="/docs/example-app/crm-tour/" className="btn btn-secondary">
            Tour the example CRM
          </Link>
        </div>
      </Section>
    </>
  );
}
