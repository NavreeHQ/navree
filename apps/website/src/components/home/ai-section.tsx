import { ArrowRightIcon } from "lucide-react";
import Link from "next/link";

import { AgentSession } from "@/components/ai/agent-session";
import { Button } from "@/components/ui/button";

/*
 * The homepage's AI section: Navree is built for the developer and for the
 * agent working next to them. What the agent gets, what the product gets,
 * and a coding agent at work through Navigator. Links to /ai/.
 */

const sides = [
  {
    title: "For your agent",
    items: [
      { name: "Agent skills", text: "one per common task", href: "/docs/ai/agent-skills/" },
      { name: "Navigator", text: "navree mcp: the running app", href: "/docs/ai/navigator/" },
      { name: "llms.txt", text: "and every page as Markdown", href: "/docs/ai/docs-for-agents/" },
      { name: "AGENTS.md", text: "in every new app", href: "/docs/ai/docs-for-agents/#agentsmd-and-claudemd" },
    ],
  },
  {
    title: "In your product",
    items: [
      { name: "AI SDK", text: "text, typed output, streaming", href: "/docs/ai/ai-sdk/" },
      { name: "Agents", text: "#[tool] methods, as the user", href: "/docs/ai/agents-and-tools/" },
      { name: "RAG", text: "embeddings over your data", href: "/docs/ai/embeddings-and-rag/" },
      { name: "MCP servers", text: "your app in any AI client", href: "/docs/ai/mcp-servers/" },
    ],
  },
];

export function AiSection() {
  return (
    <section aria-labelledby="home-ai-heading" className="container-site section-y">
      <div className="grid grid-cols-1 items-center gap-x-12 gap-y-14 lg:grid-cols-12">
        <div className="min-w-0 lg:col-span-5">
          <p className="eyebrow">AI-native</p>
          <h2 id="home-ai-heading" className="mt-5 text-display-lg font-semibold text-balance text-fg">
            Built for developers and their agents.
          </h2>
          <p className="mt-5 max-w-xl text-lg leading-relaxed text-pretty text-fg-2">
            Every Navree app has one predictable shape, so a coding agent finds its way the first time. Navree hands it the
            steps for each task and a view of the running app, and gives your product models behind one typed service.
          </p>

          <div className="mt-10 grid grid-cols-2 gap-px overflow-hidden rounded-2xl border border-line bg-line">
            {sides.map((side) => (
              <div key={side.title} className="min-w-0 bg-ink-0 p-4 sm:p-5">
                <h3 className="font-mono text-[0.6875rem] tracking-[0.12em] text-fg-3 uppercase">{side.title}</h3>
                <ul className="mt-3 grid gap-1">
                  {side.items.map((item) => (
                    <li key={item.name}>
                      <Link
                        href={item.href}
                        className="group -mx-2 flex flex-col rounded-lg px-2 py-1.5 transition-colors duration-150 hover:bg-ink-2"
                      >
                        <span className="text-[0.9375rem] font-medium text-fg transition-colors group-hover:text-accent">
                          {item.name}
                        </span>
                        <span className="text-sm text-fg-3">{item.text}</span>
                      </Link>
                    </li>
                  ))}
                </ul>
              </div>
            ))}
          </div>

          <Button asChild size="lg" variant="outline" className="mt-8 pr-1.5 pl-6">
            <Link href="/ai/" className="group">
              Navree for AI
              <span
                aria-hidden
                className="inline-flex size-9 items-center justify-center rounded-full border border-line-2 bg-ink-2 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5"
              >
                <ArrowRightIcon className="size-4" />
              </span>
            </Link>
          </Button>
        </div>

        <div className="min-w-0 lg:col-span-7">
          <AgentSession className="shadow-glow-lg" />
        </div>
      </div>
    </section>
  );
}
