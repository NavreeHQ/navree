import { ArrowRightIcon, ArrowUpRightIcon } from "lucide-react";
import Link from "next/link";

import { CommandLine } from "@/components/ai/command-line";
import { PageActions } from "@/components/docs/page-actions";
import { InlineMarkdown } from "@/components/inline-markdown";
import { BlurFade } from "@/components/magicui/blur-fade";
import { MagicCard } from "@/components/magicui/magic-card";
import { cn } from "@/lib/utils";
import { site } from "@site";

import { agentsMdExcerpt, llmsPreview, skillsData } from "./content";
import { ArrowLink, BlockHeading, HalfHeading, WindowFrame } from "./parts";

/** Navigator's tools, from /docs/ai/navigator/, grouped by what they answer. */
const navigatorTools: { group: string; tools: [string, string][] }[] = [
  {
    group: "The app",
    tools: [
      ["app_info", "Versions, features, modules, database driver"],
      ["list_routes", "Method, path, access rule, handler, module"],
      ["module_graph", "Imports, exports and providers per module"],
      ["list_providers", "Scope, owner, visibility, dependencies"],
      ["list_commands", "Every command, as navree list shows it"],
    ],
  },
  {
    group: "The data",
    tools: [
      ["describe_schema", "Tables, columns, indexes, pending migrations"],
      ["query_database", "One read-only query, at most 100 rows"],
    ],
  },
  {
    group: "What happened",
    tools: [
      ["read_logs", "Recent lines by level, target or request"],
      ["last_error", "The last error, its request and its logs"],
    ],
  },
  {
    group: "Settings and docs",
    tools: [
      ["read_config", "The merged configuration, secrets redacted"],
      ["search_docs", "These docs, for your Navree version"],
      ["read_doc", "One docs page as Markdown"],
    ],
  },
];

const setup: { n: string; title: string; command: string; prompt?: string; text: string }[] = [
  { n: "01", title: "Create an app", command: "navree new crm", text: "Writes AGENTS.md and CLAUDE.md with the app's conventions." },
  { n: "02", title: "Install the skills", command: "navree skills:install", text: "Copies every skill into .claude/skills and .agents/skills." },
  {
    n: "03",
    title: "Connect Navigator",
    command: "claude mcp add navree -- navree mcp",
    text: "Cursor, Codex and VS Code take the same command.",
  },
  {
    n: "04",
    title: "Point it at the docs",
    command: `${site.url}/llms.txt`,
    prompt: "",
    text: "Or any page's Markdown, such as /docs/basics/validation.md.",
  },
];

function SkillsBlock() {
  const { skills, files } = skillsData();
  // Two columns of rows on wide screens: fill the left column first.
  const half = Math.ceil(skills.length / 2);
  const columns = [skills.slice(0, half), skills.slice(half)];
  // Phones show the first few rows and link the full list: all of them would run to several screens.
  const onPhones = 12;

  return (
    <div className="grid grid-cols-1 gap-x-12 gap-y-12 lg:grid-cols-12">
      <div className="lg:col-span-5 lg:pt-2">
        <BlockHeading label="skills/" title="A skill for every common task" />
        <p className="mt-5 max-w-xl leading-relaxed text-pretty text-fg-2">
          A skill is a folder with a <code className="inline-code">SKILL.md</code>: when to use it, then the steps, the
          commands, a template that compiles, the checks to run and the mistakes to avoid. Agents read every description up
          front and load a skill only when the task matches, so a project carries all of them for a few lines each.
        </p>
        <p className="mt-4 max-w-xl leading-relaxed text-pretty text-fg-2">
          Two of them bring what you already have.{" "}
          <Link href="/docs/getting-started/bring-your-product/" className="link">
            Bring your product
          </Link>{" "}
          rebuilds an old product, with its data, as a Navree app.{" "}
          <Link href="/docs/getting-started/bring-your-design/" className="link">
            Bring your design
          </Link>{" "}
          turns a mock-up, an export or screenshots into Navree UI pages.
        </p>

        <dl className="mt-8 grid max-w-md grid-cols-2 gap-px overflow-hidden rounded-2xl border border-line bg-line">
          <div className="flex flex-col-reverse justify-end bg-ink-0 p-5">
            <dt className="mt-1 text-sm text-fg-3">skills, one per task</dt>
            <dd className="font-display text-display-md font-semibold text-fg numeric">{skills.length}</dd>
          </div>
          <div className="flex flex-col-reverse justify-end bg-ink-0 p-5">
            <dt className="mt-1 text-sm text-fg-3">files, checked against the code</dt>
            <dd className="font-display text-display-md font-semibold text-fg numeric">{files}</dd>
          </div>
        </dl>

        <div className="mt-8 max-w-md">
          <CommandLine command="navree skills:install" />
          <p className="mt-3 text-sm text-fg-3">
            Or <code className="inline-code">npx skills add NavreeHQ/navree</code>, or copy the folders by hand.
          </p>
        </div>

        <div className="mt-8 flex flex-wrap gap-x-7 gap-y-3">
          <ArrowLink href="/docs/ai/agent-skills/">Agent skills</ArrowLink>
          <ArrowLink href="/skills/index.json" external>
            skills/index.json
          </ArrowLink>
        </div>
      </div>

      <BlurFade inView className="min-w-0 lg:col-span-7">
        <WindowFrame
          title={`${site.domain}/skills/`}
          aside={<span className="font-mono text-xs text-fg-3 numeric">{skills.length} folders</span>}
          bodyClassName="p-1.5"
        >
          <div className="grid grid-cols-1 md:grid-cols-2 md:gap-x-1.5">
            {columns.map((column, c) => (
              <ul key={c} className={cn("grid min-w-0 grid-cols-1 content-start", c > 0 && "max-md:hidden")}>
                {column.map((skill, i) => {
                  const umbrella = skill.name === "navree";
                  return (
                    <li key={skill.name} className={cn(c === 0 && i >= onPhones && "max-md:hidden")}>
                      <a
                        href={skill.path}
                        className="group flex items-start gap-3 rounded-[0.625rem] px-3 py-2 transition-colors duration-150 hover:bg-ink-2 sm:py-2.5"
                      >
                        <span
                          aria-hidden
                          className={cn(
                            "mt-[0.55em] size-1.5 shrink-0 rounded-full",
                            umbrella ? "bg-accent shadow-[0_0_6px_rgb(61_255_143/0.8)]" : "bg-line-2 group-hover:bg-fg-3",
                          )}
                        />
                        <span className="min-w-0 flex-1">
                          <span className="flex items-center gap-2">
                            <span className="truncate font-mono text-[0.8125rem] text-fg">{skill.name}</span>
                            {umbrella ? (
                              <span className="shrink-0 rounded-full border border-accent/25 bg-accent/10 px-1.5 font-mono text-[0.6875rem] leading-4 text-accent">
                                start here
                              </span>
                            ) : null}
                          </span>
                          <span className="mt-0.5 hidden truncate text-[0.8125rem] text-fg-3 sm:block" title={skill.label}>
                            <InlineMarkdown text={skill.label} />
                          </span>
                        </span>
                        <ArrowUpRightIcon
                          aria-hidden
                          className="mt-1 size-3.5 shrink-0 text-fg-3 opacity-0 transition-opacity group-hover:opacity-100"
                        />
                      </a>
                    </li>
                  );
                })}
              </ul>
            ))}
          </div>
          <Link
            href="/docs/ai/agent-skills/#the-skills"
            className="flex items-center justify-between gap-3 rounded-[0.625rem] px-3 py-2.5 text-[0.8125rem] text-fg-2 transition-colors duration-150 hover:bg-ink-2 hover:text-fg md:hidden"
          >
            And {skills.length - onPhones} more, each with what it is for
            <ArrowRightIcon aria-hidden className="size-3.5 shrink-0 text-fg-3" />
          </Link>
        </WindowFrame>
      </BlurFade>
    </div>
  );
}

function NavigatorBlock() {
  return (
    <div className="grid grid-cols-1 gap-x-12 gap-y-12 lg:grid-cols-12">
      <div className="lg:order-2 lg:col-span-5 lg:pt-2">
        <BlockHeading label="navree mcp" title="Navigator shows your agent the running app" />
        <p className="mt-5 max-w-xl leading-relaxed text-pretty text-fg-2">
          A coding agent that reads your files still guesses at the running app. Navigator, the MCP server built into the
          CLI, answers from the app itself: the routes once every module is wired, the schema as it really is, and what
          failed in the last request.
        </p>
        <ul className="mt-6 grid max-w-xl gap-2.5 text-[0.9375rem] text-fg-1">
          {[
            "Read-only: no tool writes anything",
            "Runs on your machine over stdio, for one project",
            "Never part of your production binary",
            "Docs matched to the Navree version in your lockfile",
          ].map((point) => (
            <li key={point} className="flex gap-3">
              <span aria-hidden className="mt-[0.6em] h-px w-3 shrink-0 bg-accent" />
              {point}
            </li>
          ))}
        </ul>
        <div className="mt-8 max-w-md">
          <CommandLine command="claude mcp add navree -- navree mcp" />
          <p className="mt-3 text-sm text-fg-3">Any MCP client that starts stdio servers runs the same command.</p>
        </div>
        <ArrowLink href="/docs/ai/navigator/" className="mt-8">
          Navigator
        </ArrowLink>
      </div>

      <BlurFade inView className="min-w-0 lg:order-1 lg:col-span-7">
        <WindowFrame
          title="navree mcp · tools"
          aside={
            <span className="inline-flex items-center gap-2 font-mono text-xs text-fg-3">
              <span aria-hidden className="size-1.5 rounded-full bg-accent shadow-[0_0_6px_rgb(61_255_143/0.8)]" />
              12 tools
            </span>
          }
          bodyClassName="px-1.5 pb-1.5"
        >
          <table className="w-full border-separate border-spacing-0 text-left text-[0.8125rem]">
            <caption className="sr-only">Navigator&rsquo;s tools and what each one answers</caption>
            {navigatorTools.map((group) => (
              <tbody key={group.group}>
                <tr>
                  <th
                    colSpan={2}
                    scope="colgroup"
                    className="px-3 pt-4 pb-1.5 font-mono text-[0.6875rem] font-normal tracking-[0.12em] text-fg-3 uppercase sm:px-4"
                  >
                    {group.group}
                  </th>
                </tr>
                {group.tools.map(([name, text]) => (
                  <tr key={name} className="group">
                    <th
                      scope="row"
                      className="w-[11.5rem] rounded-l-lg px-3 py-1.5 align-top font-mono font-normal whitespace-nowrap text-fg transition-colors group-hover:bg-ink-2 max-sm:block max-sm:w-auto max-sm:rounded-r-lg max-sm:pb-0 sm:px-4"
                    >
                      {name}
                    </th>
                    <td className="rounded-r-lg px-3 py-1.5 align-top leading-snug text-fg-3 transition-colors group-hover:bg-ink-2 max-sm:block max-sm:rounded-l-lg max-sm:pt-0.5 sm:pr-4 sm:pl-0">
                      {text}
                    </td>
                  </tr>
                ))}
              </tbody>
            ))}
          </table>
        </WindowFrame>
      </BlurFade>
    </div>
  );
}

function DocsBlock() {
  const llms = llmsPreview(26);
  const agentsMd = agentsMdExcerpt(34);

  return (
    <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
      <BlurFade inView className="min-w-0">
        <MagicCard className="flex h-full flex-col rounded-[1.25rem] p-6 sm:p-8">
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div>
              <BlockHeading label="llms.txt" title="Docs written for models" />
            </div>
            <PageActions
              title="Docs for agents"
              markdownPath="/docs/ai/docs-for-agents.md"
              markdownUrl={`${site.url}/docs/ai/docs-for-agents.md`}
              className="mt-1"
            />
          </div>
          <p className="mt-5 max-w-xl leading-relaxed text-pretty text-fg-2">
            All {llms.pages} docs pages are also plain Markdown at a predictable URL, indexed in{" "}
            <code className="inline-code whitespace-nowrap">llms.txt</code> and bundled in{" "}
            <code className="inline-code whitespace-nowrap">llms-full.txt</code>.
            Every page has Copy page, and Open in Claude or ChatGPT.
          </p>
          <ul className="mt-6 flex flex-wrap gap-2">
            {[
              ["/llms.txt", "/llms.txt"],
              ["/llms-full.txt", "/llms-full.txt"],
              ["/docs/basics/validation.md", "/docs/<section>/<page>.md"],
            ].map(([href, label]) => (
              <li key={href}>
                <a
                  href={href}
                  className="inline-flex h-8 items-center gap-1.5 rounded-full border border-line-2 px-3 font-mono text-xs text-fg-1 transition-colors hover:border-fg-3 hover:bg-ink-2 hover:text-fg"
                >
                  {label}
                  <ArrowUpRightIcon aria-hidden className="size-3 text-fg-3" />
                </a>
              </li>
            ))}
          </ul>
          <div className="mt-auto pt-8">
            <WindowFrame title={`${site.domain}/llms.txt`} bodyClassName="relative">
              <pre
                aria-label="The start of llms.txt"
                className="h-[19rem] overflow-hidden px-4 py-3.5 font-mono text-[0.75rem] leading-[1.75] text-fg-2 [mask-image:linear-gradient(to_bottom,black_70%,transparent)]"
              >
                {llms.text.split("\n").map((line, i) => (
                  <span key={i} className={cn("block whitespace-pre", lineTone(line))}>
                    {line || " "}
                  </span>
                ))}
              </pre>
            </WindowFrame>
          </div>
        </MagicCard>
      </BlurFade>

      <BlurFade inView delay={0.08} className="min-w-0">
        <MagicCard className="flex h-full flex-col rounded-[1.25rem] p-6 sm:p-8">
          <BlockHeading label="AGENTS.md" title="House rules in every new app" />
          <p className="mt-5 max-w-xl leading-relaxed text-pretty text-fg-2">
            <code className="inline-code">navree new</code> writes an <code className="inline-code">AGENTS.md</code> with
            the app&rsquo;s layout, commands, generators and conventions, and a <code className="inline-code">CLAUDE.md</code>{" "}
            that imports it. Your agent knows how the app is built before its first change, and your team adds its own
            rules in one place.
          </p>
          <ul className="mt-6 flex flex-wrap gap-2 font-mono text-xs">
            {["Layout", "Commands", "Generators", "Conventions", "Before you finish"].map((item) => (
              <li key={item} className="inline-flex h-8 items-center rounded-full border border-line px-3 text-fg-2">
                {item}
              </li>
            ))}
          </ul>
          <div className="mt-auto pt-8">
            <WindowFrame
              title="crm/AGENTS.md"
              aside={<span className="font-mono text-xs text-fg-3">navree new crm</span>}
            >
              <pre
                aria-label="The start of AGENTS.md"
                className="h-[19rem] overflow-hidden px-4 py-3.5 font-mono text-[0.75rem] leading-[1.75] text-fg-2 [mask-image:linear-gradient(to_bottom,black_70%,transparent)]"
              >
                {agentsMd.split("\n").map((line, i) => (
                  <span key={i} className={cn("block whitespace-pre", lineTone(line))}>
                    {line || " "}
                  </span>
                ))}
              </pre>
            </WindowFrame>
          </div>
        </MagicCard>
      </BlurFade>
    </div>
  );
}

/** Markdown headings and quotes stand out in the file mocks. */
function lineTone(line: string): string {
  if (/^#{1,6} /.test(line)) return "text-fg font-medium";
  if (line.startsWith("> ")) return "text-fg-1";
  if (line.startsWith("```")) return "text-fg-3";
  return "";
}

function SetupRail() {
  return (
    <div>
      <div className="flex flex-wrap items-end justify-between gap-4">
        <h3 className="font-display text-display-sm font-semibold text-fg">Set up your agent in four steps</h3>
        <ArrowLink href="/docs/ai/introduction/#set-up-your-agent">The full setup</ArrowLink>
      </div>
      <ol className="mt-6 grid grid-cols-1 gap-px overflow-hidden rounded-2xl border border-line bg-line">
        {setup.map((step) => (
          <li
            key={step.n}
            className="grid grid-cols-1 items-center gap-x-10 gap-y-4 bg-ink-0 p-5 sm:p-6 md:grid-cols-[minmax(0,1fr)_minmax(0,25rem)] lg:px-8"
          >
            <div className="flex min-w-0 items-baseline gap-4">
              <span className="font-mono text-xs text-accent numeric">{step.n}</span>
              <span className="min-w-0">
                <span className="block font-medium text-fg">{step.title}</span>
                <span className="mt-1 block text-sm leading-relaxed text-pretty text-fg-3">{step.text}</span>
              </span>
            </div>
            <CommandLine
              command={step.command}
              prompt={step.prompt ?? "$"}
              label={step.prompt === "" ? "Copy URL" : "Copy command"}
            />
          </li>
        ))}
      </ol>
    </div>
  );
}

export function AgentsHalf() {
  return (
    <section id="agents" aria-labelledby="agents-title" className="cv-auto border-t border-line">
      <div className="container-site section-y">
        <HalfHeading
          id="agents-title"
          index="01"
          eyebrow="Build with agents"
          title="Your coding agent, fully briefed."
          lead={
            <p>
              An agent does its best work in code that is predictable, explicit and checked, and every Navree app is all
              three. Navree adds what an agent can&rsquo;t read from the code: the steps for each task, a view of the running
              app, and docs in the form it reads best.
            </p>
          }
        />

        <div className="mt-16 space-y-24 lg:mt-20 lg:space-y-32">
          <SkillsBlock />
          <NavigatorBlock />
          <DocsBlock />
          <SetupRail />
        </div>
      </div>
    </section>
  );
}
