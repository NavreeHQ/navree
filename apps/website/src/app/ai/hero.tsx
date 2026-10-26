import { ArrowDownIcon } from "lucide-react";

import { AgentSession } from "@/components/ai/agent-session";
import { DotPattern } from "@/components/magicui/dot-pattern";

import { PrimaryCta, SecondaryCta } from "./parts";

const halves = [
  {
    href: "#agents",
    index: "01",
    title: "Build with agents",
    items: ["Agent skills", "Navigator", "Docs for agents", "AGENTS.md"],
  },
  {
    href: "#product",
    index: "02",
    title: "Build AI into your product",
    items: ["AI SDK", "Agents and tools", "Embeddings and RAG", "MCP servers"],
  },
];

export function AiHero() {
  return (
    <section aria-labelledby="ai-title" className="relative overflow-hidden">
      <div aria-hidden className="pointer-events-none absolute inset-0">
        <DotPattern
          width={24}
          height={24}
          cr={0.9}
          className="text-line-2 [mask-image:radial-gradient(ellipse_60%_70%_at_75%_30%,black_10%,transparent_75%)] max-lg:[mask-image:radial-gradient(ellipse_120%_50%_at_50%_100%,black,transparent_80%)]"
        />
        <div className="absolute top-[-10rem] right-[-14rem] h-[46rem] w-[56rem] bg-[radial-gradient(closest-side,rgb(61_255_143/0.11),rgb(61_255_143/0.03)_55%,transparent)] max-lg:top-auto max-lg:right-[-18rem] max-lg:bottom-[-12rem] max-lg:h-[34rem] max-lg:w-[40rem]" />
      </div>

      <div className="container-site relative grid grid-cols-1 items-center gap-x-12 gap-y-14 pt-14 pb-16 sm:pt-20 lg:grid-cols-12 lg:pt-24 lg:pb-24 xl:pt-28">
        <div className="min-w-0 lg:col-span-6">
          <p className="eyebrow">Navree for AI</p>
          <h1 id="ai-title" className="mt-7 max-w-[15ch] text-display-xl font-semibold text-balance text-fg">
            AI-native, from your editor to your product.
          </h1>
          <p className="mt-7 max-w-[36rem] text-lg leading-relaxed text-pretty text-fg-2">
            Your coding agent builds with skills for every common task, Navigator to see the running app and docs written
            for models. Your product gets a typed AI SDK, agents that act as the user, answers from your own data and MCP
            servers. Both are in the box, open source and self-hosted.
          </p>
          <div className="mt-10 flex flex-wrap items-center gap-3">
            <PrimaryCta href="/docs/ai/introduction/#set-up-your-agent">Set up your agent</PrimaryCta>
            <SecondaryCta href="/docs/ai/introduction/">Read the AI docs</SecondaryCta>
          </div>
        </div>

        <div className="relative min-w-0 lg:col-span-6">
          <AgentSession className="shadow-glow-lg" />
        </div>
      </div>

      {/* The two halves of the page, as an index. */}
      <nav aria-label="On this page" className="relative border-t border-line">
        <div className="container-site">
          <ul className="grid grid-cols-1 gap-px bg-line md:grid-cols-2">
            {halves.map((half, i) => (
              <li key={half.href} className="bg-ink-0">
                <a
                  href={half.href}
                  className={`group flex h-full items-start justify-between gap-6 py-7 sm:py-9 ${i === 1 ? "md:pl-8 lg:pl-10" : "md:pr-8 lg:pr-10"}`}
                >
                  <span className="min-w-0">
                    <span className="flex items-baseline gap-3">
                      <span className="font-mono text-xs text-accent numeric">{half.index}</span>
                      <span className="font-display text-display-sm font-semibold text-fg transition-colors group-hover:text-accent">
                        {half.title}
                      </span>
                    </span>
                    <span className="mt-2.5 block pl-[calc(2ch+0.75rem)] text-[0.9375rem] text-pretty text-fg-3">
                      {half.items.join(" · ")}
                    </span>
                  </span>
                  <span
                    aria-hidden
                    className="mt-0.5 inline-flex size-9 shrink-0 items-center justify-center rounded-full border border-line-2 text-fg-2 transition-[transform,color,border-color] duration-300 ease-(--ease-out-quint) group-hover:translate-y-0.5 group-hover:border-fg-3 group-hover:text-fg"
                  >
                    <ArrowDownIcon className="size-4" />
                  </span>
                </a>
              </li>
            ))}
          </ul>
        </div>
      </nav>
    </section>
  );
}
