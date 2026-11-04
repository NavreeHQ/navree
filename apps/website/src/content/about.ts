import { site } from "../../site.config.ts";

/**
 * The words of `/about/`: who built Navree, where the name comes from, and
 * the lessons behind it. The page, its structured data and its questions
 * (`src/content/faq.ts`) read them from here.
 *
 * Every fact about Amritpal comes from him or from his GitHub profile. Add
 * nothing that he hasn't said.
 */

export const founder = {
  name: "Amritpal Singh",
  /** First name, for the sentences that use it. */
  short: "Amritpal",
  role: "a serial entrepreneur and full-stack developer",
  place: "Gurgaon, India",
  email: "boparaiamrit@gmail.com",
  github: { handle: "boparaiamrit", url: "https://github.com/boparaiamrit", since: "2011" },
  /** For the structured data: what he knows about, as search engines read it. */
  knowsAbout: ["Rust", "Laravel", "NestJS", "React", "Next.js", "Cloud computing", "DevOps", "Open-source software"],
} as const;

/** The numbers of his work, as he states them. */
export const facts: { value: string; label: string; text: string }[] = [
  { value: "15", label: "years", text: "Of building software and the companies around it" },
  { value: "6", label: "startups", text: "Built as a serial entrepreneur" },
  { value: "100+", label: "packages", text: "Written and shared as open source" },
  { value: "Millions", label: "of people", text: "Served by the products he built" },
];

/**
 * His career, one entry per role, newest first, as his LinkedIn profile has
 * it: the years (`"2019–2023"`), the role, the company and one sentence on
 * what he built there.
 *
 * The page leaves the career out while this list is empty, and shows it as a
 * timeline under "The builder" once it has entries. No code changes needed.
 */
export type CareerEntry = { years: string; role: string; company: string; text?: string };
export const career: CareerEntry[] = [];

/** One lesson of the fifteen years, and the part of Navree it became. */
export type Lesson = {
  /** The lesson, as a short imperative. */
  title: string;
  /** Why it matters, in one or two sentences. */
  lesson: string;
  /** The name of the part of Navree it became. */
  feature: string;
  /** What that part does. */
  text: string;
  link: { href: string; label: string };
  /**
   * The rules on `/architecture/` that show this lesson in the code (the ids
   * of their cards). The lesson links to the first one the page shows, and
   * each of them links back here.
   */
  rules?: string[];
};

/** The id of a lesson's row on `/about/`, from its part of Navree: `lesson-lazy-by-default`. */
export const lessonId = (lesson: Lesson) =>
  `lesson-${lesson.feature
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "")}`;

export const lessons: Lesson[] = [
  {
    title: "Give every app one clear structure.",
    lesson: "The codebases that lasted were the ones a new person could find their way around on the first day.",
    feature: "Modules",
    text: "Every feature is a module that says what it imports, provides and exports. The app refuses to start when a piece is missing, and names it.",
    link: { href: "/docs/architecture/modules/", label: "Modules" },
  },
  {
    title: "Let the compiler catch the mistake.",
    lesson: "A mistake found while typing costs a minute. The same mistake found by a customer costs a week.",
    feature: "Typed from end to end",
    text: "Requests, queries, jobs and configuration are typed, so comparing a money column with a piece of text doesn't compile.",
    link: { href: "/docs/orm/models/#mistakes-caught-at-compile-time", label: "Mistakes caught at compile time" },
  },
  {
    title: "Make safe the default.",
    lesson: "Security that depends on remembering a setting gets forgotten on the busiest day.",
    feature: "Secure by default",
    text: "A route that doesn't say who may call it doesn't compile. The code that reads untrusted input, from the router and the SQL to tokens, mail and files, is Navree's own, with limits and fuzz tests.",
    link: { href: "/docs/prologue/release-notes/#security-hardening", label: "Security hardening in 1.0" },
    rules: ["no-injection"],
  },
  {
    title: "Load only what the screen needs.",
    lesson: "At real scale, the cost is in everything that is loaded and never used.",
    feature: "Lazy by default",
    text: "Lists come a page at a time and large results stream in constant memory. The DataTable sorts and pages on the server, and a page loads script only for what it uses.",
    link: { href: "/docs/orm/pagination-and-streaming/", label: "Pagination and streaming" },
    rules: ["lazy-by-default"],
  },
  {
    title: "Give the server the last word.",
    lesson: "A browser can be switched off, edited or skipped. A rule that lives only there is a suggestion.",
    feature: "The server decides",
    text: "Every rule a Navree UI form shows in the browser runs again on the server, and the form works with JavaScript off.",
    link: { href: "/docs/ui/forms/", label: "Forms" },
    rules: ["server-authority", "refused-forms"],
  },
  {
    title: "Plan for teams from the start.",
    lesson: "Sooner or later every product serves teams, and adding them late means touching every query.",
    feature: "Workspaces built in",
    text: "Members, roles, invitations, and data kept apart by row, schema or database, in the default features: add WorkspaceModule to your app, or start from the saas or api starter kit, which adds it for you.",
    link: { href: "/docs/workspaces/introduction/", label: "Workspaces" },
  },
  {
    title: "Let a team draw screens with what it knows.",
    lesson: "Teams arrive with the skills they have, and the screens are where a product meets its users.",
    feature: "Screens your way",
    text: "Draw your screens in Rust with Navree UI, or put a Next.js or Nuxt app beside the same API.",
    link: { href: "/docs/ui/choosing-a-ui/", label: "Choosing a UI" },
  },
  {
    title: "Ship the tools to run it.",
    lesson: "Monitoring, an admin panel and a queue dashboard are needed from the first customer, not after the launch.",
    feature: "First-party tools",
    text: "Lighthouse for monitoring, Helm for admin, Fleet for queues and Logbook for debugging: open source, and served by your own app.",
    link: { href: "/ecosystem/", label: "The ecosystem" },
  },
  {
    title: "Teach the agent as well as the person.",
    lesson: "Code is now written by people and coding agents together, and both have to learn the framework.",
    feature: "AI-native",
    text: "Agent skills, the Navigator MCP server and every docs page as Markdown, so your agent writes Navree the way the docs do.",
    link: { href: "/docs/ai/agent-skills/", label: "Agent skills" },
  },
];

/** The picture's text for screen readers (its `alt`), and its caption. */
export const picture = {
  alt:
    "Seen from behind and a little to the side, a man with short, thick hair and a full beard, nothing on his head, sits in a mesh office chair at a desk, " +
    "his right hand on the keyboard, facing a glowing monitor in the middle of the picture with Navree code on its screen. " +
    "His young son, a small Sikh boy with his hair in a topknot under a glowing white patka, sits on his lap and watches the screen. " +
    "His daughter, older and taller, stands on the right with her hair in two long, wavy pigtails, one hand on the desk, watching and learning. " +
    "A plant and a mug sit at the right end of the desk. Everything is drawn in glowing green lines, the clothes are made of columns of 0 and 1 digits, " +
    "and digits fall like rain at the left and right edges.",
  caption: `${founder.short} at his desk, Navraj on his lap and Navreet beside him, drawn in light and ones and zeros.`,
};

/** Where to report a problem in Navree itself. */
export const issues = `${site.repoUrl}/issues`;
