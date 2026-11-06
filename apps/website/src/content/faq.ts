import { site } from "../../site.config.ts";
import { founder } from "./about.ts";

/**
 * The questions the site answers on a page, as data. `Faq` (src/components/
 * faq.tsx) draws the visible block and the `FAQPage` data from one of these
 * sets, so the two can't differ. Rules for every answer:
 *
 * - answer first, in 40 to 80 words (`scripts/check-seo.ts` counts them);
 * - true on `main`: each fact is in the docs or the code, and a number that
 *   can change is computed here from the file that owns it;
 * - plain text with `code` spans only: it goes into structured data as is;
 * - the voice of the launch copy: short sentences, "you", no filler.
 *
 * `link` is optional and only drawn on the page, after the answer.
 */
export type FaqItem = {
  question: string;
  answer: string;
  link?: { href: string; label: string };
};

export type FaqSet = {
  /** The page the questions belong to, with its trailing slash. */
  path: string;
  heading: string;
  items: FaqItem[];
};

const license = site.license.replace(" OR ", " or ");

export const homeFaq: FaqSet = {
  path: "/",
  heading: "Questions about Navree",
  items: [
    {
      question: "What is Navree?",
      answer:
        "Navree is an open-source Rust framework for complete applications: the backend, the screens and the tools to run them. Features live in modules, a dependency-injection container wires them when the app boots, and auth, workspaces, an ORM, queues, search, real time and AI come in the box. A whole application compiles to one binary.",
      link: { href: "/docs/prologue/introduction/", label: "Read the introduction" },
    },
    {
      question: "Is Navree free and open source?",
      answer: `Yes. Navree is free and open source under the ${license} license, your choice. There are no paid tiers, no seats and no hosted plan: you run it yourself. The source is on GitHub in the NavreeHQ/navree repository, and the docs on this site cover everything that ships.`,
      link: { href: site.repoUrl, label: "See the repository" },
    },
    {
      question: "How do I install Navree?",
      answer: `Install Rust ${site.rustVersion} or newer, then run \`${site.install.cli}\` to get the \`navree\` command. Create an app with \`${site.install.newApp}\`. The crates are not on crates.io yet, so installs come from Git until they are published.`,
      link: { href: site.getStarted, label: "Follow the installation guide" },
    },
    {
      question: "Which databases does Navree support?",
      answer:
        "TimescaleDB, which runs on PostgreSQL, is the default for new apps. The ORM and its migrations also work with PostgreSQL, MySQL, SQLite and MongoDB. MariaDB connects through the MySQL driver but is untested until its test suite runs on a MariaDB server. For SQL databases the ORM writes its own SQL. `navree new --database sqlite` makes an app that needs no database server.",
      link: { href: "/docs/orm/getting-started/", label: "Read about the ORM" },
    },
    {
      question: "Can I use Next.js or Nuxt with Navree?",
      answer:
        "Yes. Draw your screens with Navree UI in Rust, or put a Next.js (React) or Nuxt (Vue) app beside the same API. `navree new --ui next` writes a Next.js app in `web/` with its own sign-in. Add `@navree/next` for a typed client, a DataTable and live channels. Nuxt uses the `@navree/nuxt` module and has no starter kit yet.",
      link: { href: "/docs/ui/choosing-a-ui/", label: "Choose a UI" },
    },
    {
      question: "Does Navree work with AI coding agents?",
      answer:
        "Yes. Navree ships agent skills that you install with `navree skills:install`, Navigator (an MCP server you start with `navree mcp`), and every docs page as Markdown, plus `llms.txt` and `llms-full.txt`. Two skills bring an existing product or design into Navree.",
      link: { href: "/ai/", label: "See Navree for AI" },
    },
  ],
};

/** What the Navree UI questions are computed from: the catalogue's groups, the "Coming next" table and the example apps, read where they are owned. */
export type UiFaqInput = {
  groups: { title: string; components: unknown[] }[];
  roadmap: { text: string; status: string }[];
  /** The apps of `src/app/ui/showcases.ts`, in order. */
  apps: { name: string; kind: string; ui: string; hardest: string }[];
};

/** `a, b and c`; `a, x; b, y; and c, z` when the items hold commas themselves. */
function andList(items: string[]): string {
  if (items.length < 2) return items.join("");
  const semi = items.some((item) => item.includes(","));
  return `${items.slice(0, -1).join(semi ? "; " : ", ")}${semi ? "; and " : " and "}${items.at(-1)}`;
}
/** `A store's back office` as `a store's back office`. */
const inSentence = (text: string) => text.charAt(0).toLowerCase() + text.slice(1);

/** "What can I build with Navree UI?", answered from the apps themselves. */
function whatYouCanBuild(apps: UiFaqInput["apps"]): string {
  const drawn = apps.filter((app) => app.ui === "Navree UI");
  const others = apps.filter((app) => app.ui !== "Navree UI");
  // One app gets its hardest fact; several are named, so the answer stays under 80 words.
  const named = drawn.map((app) => `${app.name}, ${inSentence(app.kind)}${drawn.length === 1 ? ` (${app.hardest})` : ""}`);
  return [
    "Whole products: tables over lakhs of rows, drawers over their pages, forms the server checks again and live feeds.",
    `The repository has ${drawn.length === 1 ? "an example app" : `${drawn.length} example apps`} drawn with Navree UI: ${andList(named)}.`,
    ...others.map((app) => `${app.name} is ${inSentence(app.kind)} on a Navree API, with ${app.ui.replace(" · ", " and ")} front ends.`),
    "Each one runs on your machine, with its code in the repository.",
  ].join(" ");
}

/** Navree UI: the counts come from the catalogue and the roadmap that own them. */
export function uiFaq({ groups, roadmap, apps }: UiFaqInput): FaqSet {
  const count = groups.reduce((n, g) => n + g.components.length, 0);
  const names = groups.map((g) => g.title.toLowerCase());
  const by = (status: string) => roadmap.filter((item) => item.status === status).length;
  const shipped = roadmap.filter((item) => item.status === "Shipped");
  return {
    path: "/ui/",
    heading: "Questions about Navree UI",
    items: [
      {
        question: "What is Navree UI?",
        answer:
          "Navree UI is the screen toolkit in Navree (`navree::ui`, the `ui` feature, on by default). You write typed Rust components, the server renders them to HTML in the same binary as your API, and one stylesheet and a small runtime make them interactive. A strict content security policy applies to every page.",
        link: { href: "/docs/ui/overview/", label: "Read how it works" },
      },
      {
        question: "What can I build with Navree UI?",
        answer: whatYouCanBuild(apps),
        link: { href: "/showcase/", label: "See every example app" },
      },
      {
        question: "How many components does Navree UI have?",
        answer: `The docs catalogue lists ${count} components in ${groups.length} groups: ${names.slice(0, -1).join(", ")} and ${names.at(-1)}. Some entries are families, such as Stack, Row and Grid. Each one has a live preview, its parts, its props and its keyboard notes. The count is read from the catalogue when the site is built.`,
        link: { href: "/docs/components/overview/", label: "Browse the components" },
      },
      {
        question: "Does Navree UI need JavaScript?",
        answer:
          "Not for most of a page. A page with nothing interactive ships no script. Links, forms, dialogs, accordions, switches and the DataTable's sorting, filtering and paging all work with JavaScript off. A small runtime loads only for what the markup uses, such as popovers, tooltips and replies that update part of a page.",
        link: { href: "/docs/ui/overview/", label: "See what ships to the browser" },
      },
      {
        question: "Can I use Navree UI inside Next.js or Nuxt?",
        answer:
          "No. Navree UI is Rust that renders HTML on the server. A Next.js or Nuxt app draws its own screens and calls the same Navree API, with `@navree/next` or `@navree/nuxt` for sign-in, data and live updates. Choosing a UI says what each costs and gives.",
        link: { href: "/docs/ui/choosing-a-ui/", label: "Choose a UI" },
      },
      {
        question: "Is Navree UI finished?",
        answer: `Not yet. The components in the catalogue work today. Of the ${roadmap.length} modules planned, ${shipped.length} ${shipped.length === 1 ? "has" : "have"} shipped, ${by("Building")} are being built, ${by("Next")} are next and ${by("Later")} come later. The latest to ship: ${shipped.slice(-4).map((item) => item.text.split(":")[0]).join("; ")}.`,
        link: { href: "/docs/ui/coming-next/", label: "Read Coming next" },
      },
      {
        question: "Can I turn an existing design into Navree UI pages?",
        answer:
          "Yes. The `navree-design-to-ui` skill reads an HTML mock-up, a design-tool export or screenshots, and your coding agent builds Navree UI pages from them, one module at a time. The skill stops at set points, and you confirm the result at each one, with the pages shown beside the original design. The Bring your design page lists the phases.",
        link: { href: "/docs/getting-started/bring-your-design/", label: "Bring your design" },
      },
    ],
  };
}

export const aiFaq: FaqSet = {
  path: "/ai/",
  heading: "Questions about Navree for AI",
  items: [
    {
      question: "What does AI-native mean for Navree?",
      answer:
        "It means two things. Your coding agent builds with Navree through skills, the Navigator MCP server and docs written for models. And your product uses models through the AI SDK, agents, RAG and MCP servers. Both halves are in the same framework and documented together.",
      link: { href: "/docs/ai/introduction/", label: "Read the AI introduction" },
    },
    {
      question: "What is Navigator?",
      answer:
        "Navigator is Navree's MCP server for coding agents. You start it with `navree mcp`. It gives your agent your app's routes, module graph, providers, database schema, read-only SQL, logs and configuration, and the docs for the Navree version your app uses.",
      link: { href: "/docs/ai/navigator/", label: "Set up Navigator" },
    },
    {
      question: "How do I install the agent skills?",
      answer:
        "Run `navree skills:install` in your project, or `npx skills add NavreeHQ/navree`. Each skill is a step-by-step guide for coding agents, one per common task, such as validation or workspaces. The skills also live in the repository's `skills/` directory and are listed in an index on this site.",
      link: { href: "/docs/ai/agent-skills/", label: "See the agent skills" },
    },
    {
      question: "Can an AI assistant read the Navree docs?",
      answer: `Yes. Every docs page is also a Markdown file: replace the trailing slash of its URL with \`.md\`. \`llms.txt\` indexes them, and \`llms-full.txt\` holds all of them in one file. Navigator serves the docs of the Navree version your app uses.`,
      link: { href: "/docs/ai/docs-for-agents/", label: "Docs for agents" },
    },
    {
      question: "Can I add AI features to my own product?",
      answer:
        "Yes. The AI SDK is one `Ai` service for Anthropic, OpenAI, Google Gemini, Mistral, Ollama and AWS Bedrock: text, typed structured output, streaming, failover between models, token budgets and `AiFake` for tests. Agents, RAG and MCP servers are part of the framework too.",
      link: { href: "/docs/ai/ai-sdk/", label: "Read the AI SDK docs" },
    },
    {
      question: "How do I bring an existing product or design into Navree?",
      answer:
        "Use the two skills. `navree-reverse-engineering` reads an old product and its data and builds specs, tests and a Navree app. `navree-design-to-ui` turns a design into Navree UI pages. You run both with your coding agent, and each stops for your confirmation at set points.",
      link: { href: "/docs/getting-started/bring-your-product/", label: "Bring your product" },
    },
  ],
};

/** The architecture page (`/architecture/`): the shapes, the lists, the script and the page's safety. */
export const architectureFaq: FaqSet = {
  path: "/architecture/",
  heading: "Questions about Navree's architecture",
  items: [
    {
      question: "Can I start as a monolith and split later?",
      answer:
        "Yes. `navree new` writes one binary, and each feature is a module that keeps its services, routes, access rules, migrations and tests wherever it runs. Add `WorkspaceModule` when you serve many customers. When one module needs its own deploys, it moves into a service of its own with `MicroModule`, and its callers send a message instead of calling a method.",
      link: { href: "#grow", label: "Grow without rewriting" },
    },
    {
      question: "How does Navree keep lists fast with millions of rows?",
      answer:
        "By reading one page at a time. `cursor_paginate` pages by keyset: it reads the rows after the last one seen, with no count and no offset, so a deep page costs what the first does. The DataTable sorts, filters and pages on the server, can cap its count or skip it, and reads a deep numbered page from the nearer end of the index.",
      link: { href: "/docs/orm/pagination-and-streaming/#cursor-pagination", label: "Read about cursor pagination" },
    },
    {
      question: "Does Navree UI need JavaScript?",
      answer:
        "Not for most of a page. The server renders pages to HTML, and a page with nothing interactive ships no script. Links, forms, dialogs and the DataTable's sorting, filtering and paging work with JavaScript off. Script comes in small modules, and a page loads only the ones its markup uses, such as the table module for a DataTable or the live module for a region that follows events.",
      link: { href: "/docs/ui/overview/#what-ships-to-the-browser", label: "See what ships to the browser" },
    },
    {
      question: "How does Navree stop script injection in pages?",
      answer:
        "With a strict content security policy on every page: scripts, styles and requests from the page's own origin only, and no inline script or style at all. Text goes through one escaper, every URL through `Href`, which refuses `javascript:`, and elements that load or run something are refused. The runtime reads the page through the DOM's own prototypes, so a field named to trick it changes nothing.",
      link: { href: "/docs/ui/overview/#security", label: "Read how pages stay safe" },
    },
  ],
};

/** The introduction of the docs (`/docs/prologue/introduction/`). */
export const introductionFaq: FaqSet = {
  path: "/docs/prologue/introduction/",
  heading: "Common questions",
  items: [
    {
      question: "How is a Navree app structured?",
      answer:
        "Features live in modules. Each module declares what it imports, provides and exports, and a dependency-injection container wires them together when the app boots. If a dependency is missing or private, the app refuses to start and names the module and the type. A whole application compiles to one binary.",
      link: { href: "/docs/architecture/modules/", label: "Read about modules" },
    },
    {
      question: "What do I need to run Navree?",
      answer: `Rust ${site.rustVersion} or newer and the \`navree\` command-line tool. You also need a C compiler; on Windows that means the MSVC build tools. New apps run on TimescaleDB, which \`navree dock up\` starts in Docker or Podman. An app made with \`--database sqlite\` needs no database server.`,
      link: { href: "/docs/getting-started/installation/", label: "Install Navree" },
    },
    {
      question: "What is Navree built on?",
      answer:
        "Navree runs on Tokio, Hyper and Tower. It has its own router, ORM and validation rules, speaks TLS through rustls and hashes passwords with Argon2id. The ORM writes its own SQL and runs it on sqlx connections. You write your application against Navree's modules and extractors.",
      link: { href: "/docs/architecture/crate-hierarchy/", label: "See the crate hierarchy" },
    },
    {
      question: "How do I choose how to draw my screens?",
      answer:
        "Pick Navree UI to render typed Rust components on the server in the same binary as your API. Pick Next.js (React) or Nuxt (Vue) to keep a separate front end beside the API. All three use the same API, the same access rules and the same dark theme.",
      link: { href: "/docs/ui/choosing-a-ui/", label: "Choosing a UI" },
    },
    {
      question: "Where do I start?",
      answer:
        "Install the CLI, then build the task-list API in Your first app, which takes you from `navree new` to green tests. When you reach the screens, read Choosing a UI. For the ideas behind modules and providers, read the Architecture concepts section.",
      link: { href: "/docs/getting-started/your-first-app/", label: "Build your first app" },
    },
  ],
};

/** The about page (`/about/`): who built Navree, and where the name comes from. */
export const aboutFaq: FaqSet = {
  path: "/about/",
  heading: "Questions about who built Navree",
  items: [
    {
      question: "Who built Navree?",
      answer: `${founder.name}, a serial entrepreneur and full-stack developer in ${founder.place}. He has spent fifteen years building software: six startups, products that, by his own estimate, have served millions of people, and more than a hundred open-source packages, with Laravel, NestJS, React, Next.js, the cloud and DevOps. Navree puts what that work taught him into one framework, written in Rust.`,
      link: { href: founder.github.url, label: `${founder.short} on GitHub` },
    },
    {
      question: "Where does the name come from?",
      answer: `From ${founder.name}'s daughter, Navreet: Navree is her name without its last letter. The picture on this page shows her beside her father at his desk, watching the screen and learning, with her younger brother Navraj on their father's lap. All three are drawn in ones and zeros, like the screen they are looking at.`,
    },
    {
      question: "Why was Navree built?",
      answer:
        "To put fifteen years of lessons into one framework. Across six startups the same needs came back: one clear structure, mistakes caught by the compiler, security on by default, workspaces for teams, screens in the stack a team knows, the tools to run a product and help for coding agents. Navree builds each of them in, so a new product starts with them.",
      link: { href: "#lessons", label: "Read the lessons" },
    },
    {
      question: "How can I contact the person who built Navree?",
      answer: `Write to ${founder.short} at ${founder.email}, or find him on GitHub as ${founder.github.handle}. To report a bug or ask for a feature, open an issue in the NavreeHQ/navree repository, so everyone with the same question can follow the answer. Report a security problem privately to security@navree.com instead, as the repository's security policy asks.`,
      link: { href: `mailto:${founder.email}`, label: `Email ${founder.short}` },
    },
  ],
};

/** What the performance questions are computed from: the result file, and the screens the example apps measured. */
export type PerformanceFaqInput = {
  /** Requests a second of the authenticated, paginated list and of the validated insert. */
  listRps: number;
  createRps: number;
  cpus: number;
  peakMemMb: number;
  startupMs: number;
  runs: number;
  durationS: number;
  warmupS: number;
  connections: number;
  /** Checks the conformance test passed before the run, or null. */
  checks: number | null;
  /** The day the run started, written out: "26 October 2026". */
  measured: string;
  /** The store's orders page: its orders (written out) and its paint on a laptop in milliseconds. */
  store: { orders: string; paintMs: number };
};

/** The performance page (`/performance/`): how fast, how it was measured, and what the numbers mean. */
export function performanceFaq(input: PerformanceFaqInput): FaqSet {
  const n = (value: number) => Math.round(value).toLocaleString("en-IN");
  return {
    path: "/performance/",
    heading: "Questions about Navree's speed",
    items: [
      {
        question: "How fast is Navree?",
        answer: `On a CRM-style API with JWT auth, validation and PostgreSQL, one Navree binary on ${input.cpus} CPUs serves ${n(input.listRps)} authenticated, paginated lists a second and ${n(input.createRps)} validated inserts a second. It peaks at ${input.peakMemMb} MB and answers its first request ${input.startupMs} ms after it starts. Screens paint fast too: the store's orders page in ${input.store.paintMs} ms over ${input.store.orders} orders.`,
        link: { href: "#results", label: "See the results" },
      },
      {
        question: "How was Navree measured?",
        answer: `In three containers on one machine with fixed CPUs: Navree, PostgreSQL and the load generator wrk. Each test is the median of ${input.runs} runs of ${input.durationS} seconds at ${input.connections} open connections, after a ${input.warmupS}-second warm-up${input.checks ? `, and the app first passes a ${input.checks}-check conformance test` : ""}. The run started on ${input.measured}.`,
        link: { href: "#method", label: "Read the method" },
      },
      {
        question: "Do the figures include a real database and real work?",
        answer:
          "Yes, except for the plain JSON test, which has no database on purpose. The other tests go through routing, JWT authentication, validation, the ORM and a PostgreSQL query, and the inserts run in a transaction. They measure the overhead of the framework on one small app, so read them as a guide and not as a promise for your product.",
        link: { href: "#workload", label: "See the workload" },
      },
      {
        question: "Can I repeat the run on my own machine?",
        answer:
          "Yes. You need Docker and Node.js. One script starts PostgreSQL, builds the load generator and the app, runs the conformance test, measures every test and writes the result file this page is built from. Every setting is a flag, so you can fit it to your CPUs. The raw result files are on the page.",
        link: { href: "#reproduce", label: "Run it yourself" },
      },
    ],
  };
}

/** The set drawn at the end of a docs page (`prologue/introduction`), or null. */
export function docFaq(slug: string): FaqSet | null {
  return slug === "prologue/introduction" ? introductionFaq : null;
}

/** An answer as plain text, for structured data and Markdown: code marks removed. */
export const plainAnswer = (text: string) => text.replace(/`/g, "");

/** The words of an answer, as `scripts/check-seo.ts` counts them. */
export const wordCount = (text: string) => plainAnswer(text).split(/\s+/).filter(Boolean).length;
