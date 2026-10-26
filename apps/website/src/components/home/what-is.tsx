import { DotPattern } from "@/components/magicui/dot-pattern";

import { ModuleGraph } from "./module-graph";
import { ArrowLink, HomeSection } from "./parts";

const principles = [
  { term: "#[module]", text: "Every feature is a module with its routes, services and tables." },
  { term: "imports · exports", text: "Each module states what it uses and what it shares." },
  { term: "navree check", text: "The whole graph is validated before anything is built." },
];

/** "What is Navree": the philosophy beside the module graph. */
export function WhatIs() {
  return (
    <HomeSection id="what-is">
      <div className="grid gap-x-16 gap-y-14 xl:grid-cols-12 xl:items-center">
        <div className="xl:col-span-5">
          <p className="eyebrow">What is Navree</p>
          <h2 id="what-is" className="mt-5 text-display-lg font-semibold text-balance text-fg">
            One clear structure for every backend.
          </h2>
          <div className="mt-6 max-w-2xl space-y-5 text-pretty">
            <p className="text-lg leading-relaxed text-fg-1">
              Navree is a framework for building efficient, secure and scalable server-side applications in Rust. It gives you an
              application architecture out of the box: features live in modules, modules declare what they import and export, and
              a dependency-injection container wires them together when the app boots, refusing to start if anything is missing.
            </p>
            <p className="leading-relaxed text-fg-2">
              Around that structure sits a complete standard library for backends, from routing and validation to workspaces,
              queues, monitoring and AI, designed together, documented together and released together. The result is code that
              stays easy to test, easy to change and easy to hand to a new teammate, or to an AI agent.
            </p>
          </div>

          <dl className="mt-9 max-w-2xl divide-y divide-line border-y border-line">
            {principles.map((p) => (
              <div key={p.term} className="grid gap-1 py-3 sm:grid-cols-[9.5rem_minmax(0,1fr)] sm:gap-5">
                <dt className="font-mono text-[0.8125rem] text-fg">{p.term}</dt>
                <dd className="text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{p.text}</dd>
              </div>
            ))}
          </dl>

          <ArrowLink href="/docs/prologue/introduction/" className="mt-8">
            Read the introduction
          </ArrowLink>
        </div>

        <figure className="bezel min-w-0 xl:col-span-7">
          <div className="bezel-core relative overflow-hidden px-4 py-6 sm:px-8 sm:py-9">
            <DotPattern
              width={16}
              height={16}
              className="text-line [mask-image:radial-gradient(ellipse_at_center,black_30%,transparent_75%)]"
            />
            <div className="relative">
              <ModuleGraph />
            </div>
          </div>
          <figcaption className="px-3 pt-3 pb-1.5 text-sm leading-relaxed text-fg-3">
            Platform modules export their providers. <span className="font-mono text-[0.8125rem] text-fg-2">AppModule</span>{" "}
            imports every module, and the container injects what each one declares when the app boots.
          </figcaption>
        </figure>
      </div>
    </HomeSection>
  );
}
