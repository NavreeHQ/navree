import { execFileSync } from "node:child_process";

import { site } from "../../site.config.ts";

/**
 * The changelog: what each release adds and fixes, grouped by week, each week
 * opening with a highlight of two lines, and each week's changes under the
 * area of Navree they touch. Navree 1.0 is the first release, so it is the
 * first week; a later release adds its week to `WEEKS`.
 *
 * It is written, not copied from the commit log: a visitor wants to know what
 * changed for them, and a commit subject is written for the people who maintain
 * the code. A change in an internal scope (the proof apps, the docs generator,
 * the website itself) is never shown, whoever adds it.
 */

/** The parts of Navree a change belongs to, in the order the page lists them. */
export const AREAS = ["Framework", "Navree UI", "Tools", "Packages", "AI", "CLI"] as const;
export type Area = (typeof AREAS)[number];

export type Change = {
  /** `feat` or `fix`. */
  type: "feat" | "fix";
  area: Area;
  /** The part in parentheses of the commit this came from, or "". */
  scope: string;
  /** What changed, in a sentence a user of Navree can read. */
  subject: string;
  /** Where to read more. */
  link: { href: string; label: string };
  /** True for a breaking change. */
  breaking: boolean;
};

/** A release inside a week: its version, the commit it was tagged on, and when. */
export type Release = {
  version: string;
  /** Full commit id. */
  hash: string;
  /** The release's date with its offset, ISO 8601. */
  date: string;
  /** The release's day, `2026-11-03`, in the committer's time zone. */
  day: string;
};

/** One week of the changelog as it is written: `start` is the Monday. */
export type WeekEntry = {
  start: string;
  /** Two lines at most (`HIGHLIGHT_MAX` characters): what the week was about. */
  highlight: string;
  releases: Release[];
  changes: Change[];
};

/** A week as the page draws it: its changes under their areas. */
export type Week = {
  start: string;
  highlight: string;
  releases: Release[];
  areas: { name: Area; changes: Change[] }[];
  count: number;
  /** The newest release's date, for the feed. */
  updated: string;
};

/** Scopes of apps and crates that have left the repository; none so far. */
export const RETIRED_SCOPES: readonly string[] = [];

/** Scopes of work that is not Navree itself: the proof apps, the docs generator, the website. Never shown, in the page or in the feed. */
export const INTERNAL_SCOPES: readonly string[] = ["ui-proof", "ui-table-proof", "ui-docs", "website", "site", "e2e", "ci", "deps", "campaign-demo"];

/** The longest a week's highlight may be: about two lines at the page's width. */
export const HIGHLIGHT_MAX = 220;

/** The sections of the release notes a change is told in: their anchors and names. */
const NOTES = "/docs/prologue/release-notes/";
const SECTIONS = {
  foundation: "Foundation",
  http: "HTTP",
  data: "Data",
  security: "Security",
  workspaces: "Workspaces",
  "background-work": "Background work",
  "navree-ui": "Navree UI",
  tools: "Tools",
  packages: "Packages",
  "ai-native": "AI-native",
  "the-cli": "The CLI",
} as const;
const feat = (area: Area, subject: string, anchor: keyof typeof SECTIONS): Change => ({
  type: "feat",
  area,
  scope: "",
  subject,
  link: { href: `${NOTES}#${anchor}`, label: `${SECTIONS[anchor]} in the release notes` },
  breaking: false,
});

/** Every week, newest first. */
const WEEKS: WeekEntry[] = [
  {
    start: "2026-11-02",
    highlight:
      "Navree 1.0, the first release: the framework, Navree UI for your screens, the first-party tools and skills for your coding agent, free to self-host.",
    releases: [
      {
        version: "1.0.0",
        hash: "124d1fa8b90db30def87d0e1dd4c6d57c43a89f7",
        date: "2026-11-03T19:31:00+05:30",
        day: "2026-11-03",
      },
    ],
    changes: [
      feat("Framework", "Modules and dependency injection: the app's graph is checked before anything is built, and a route with no access declaration doesn't compile.", "foundation"),
      feat("Framework", "An HTTP server with a default pipeline, routes denied until you allow them, one JSON error shape and limits on what any client can hold.", "http"),
      feat("Framework", "An ORM, a schema builder and migrations for PostgreSQL, MySQL and SQLite, with MongoDB beside them.", "data"),
      feat("Framework", "Sign-in with rotating refresh tokens and two-factor codes, roles and abilities, and an audit log.", "security"),
      feat("Framework", "Workspaces: members, roles, invitations and tenants kept apart by row, schema or database.", "workspaces"),
      feat("Framework", "Queues, events, schedules, mail, notifications and real time.", "background-work"),
      feat("Navree UI", "Typed components that the server renders, each with a live preview in the docs.", "navree-ui"),
      feat("Navree UI", "Forms from a validated struct, an app shell and a DataTable whose sort, filters and page live in the address.", "navree-ui"),
      feat("Navree UI", "Charts and reports, selects and comboboxes, date and time pickers, a wizard, drag and drop, uploads and a live hub.", "navree-ui"),
      feat("Navree UI", "Themes and a theme builder that refuses a theme whose text fails the contrast check.", "navree-ui"),
      feat("Navree UI", "A strict content security policy and little script: a page with nothing interactive ships none.", "navree-ui"),
      feat("Tools", "Lighthouse for monitoring, Logbook for a local timeline of every request and Fleet for queues.", "tools"),
      feat("Tools", "Helm, an admin panel generated from your models and your access rules.", "tools"),
      feat("Tools", "Harbor for servers and zero-downtime releases from the CLI, and Dock for local services.", "tools"),
      feat("Packages", "Search, billing, OAuth and OpenID Connect, single sign-on, directory sync and custom domains.", "packages"),
      feat("Packages", "Webhooks, integrations, documents in PDF and DOCX, media, imports and exports, and workflows.", "packages"),
      feat("Packages", "Microservices over TCP, NATS, Kafka, RabbitMQ, Redis Streams and MQTT, with gRPC and GraphQL.", "packages"),
      feat("AI", "An AI SDK with agents and tools, embeddings and failover across providers, and MCP servers for your own app.", "ai-native"),
      feat("AI", "Agent skills for every common task, and two that bring an existing product or design into Navree.", "ai-native"),
      feat("AI", "Navigator, an MCP server that shows your coding agent the running app, and every docs page as Markdown.", "ai-native"),
      feat("CLI", "navree new with starter kits and a Navree UI front end, generators for modules, pages, forms and tables, and navree dev.", "the-cli"),
    ],
  },
];

/** The Monday of the week a day falls in, as `2026-11-02`. */
export function weekStart(day: string): string {
  const [y, m, d] = day.split("-").map(Number);
  const date = new Date(Date.UTC(y, m - 1, d));
  date.setUTCDate(date.getUTCDate() - ((date.getUTCDay() + 6) % 7));
  return date.toISOString().slice(0, 10);
}

/** Every week, newest first, with its changes under their areas and the internal ones left out. */
export function changelog(): Week[] {
  return [...WEEKS]
    .sort((a, b) => (a.start < b.start ? 1 : -1))
    .map((week) => {
      const shown = week.changes.filter((change) => !INTERNAL_SCOPES.includes(change.scope));
      return {
        start: week.start,
        highlight: week.highlight,
        releases: week.releases,
        areas: AREAS.map((name) => ({ name, changes: shown.filter((change) => change.area === name) })).filter((area) => area.changes.length > 0),
        count: shown.length,
        updated: week.releases.reduce((a, b) => (a.date > b.date ? a : b)).date,
      };
    });
}

/** `3 November 2026`, from `2026-11-03`, without a time zone shifting the day. */
export function formatDay(day: string): string {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(Date.UTC(y, m - 1, d)).toLocaleDateString("en-GB", { day: "numeric", month: "long", year: "numeric", timeZone: "UTC" });
}

export const commitUrl = (hash: string) => `${site.repoUrl}/commit/${hash}`;

/** The commit the site is built from and its date, for the line that says how fresh the page is. */
export function buildInfo(cwd: string = process.cwd()): { sha: string; date: string } {
  try {
    const [sha, date] = execFileSync("git", ["log", "-1", "--format=%h%x1f%cI", "HEAD"], {
      cwd,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    })
      .trim()
      .split("\x1f");
    if (sha && date) return { sha, date };
  } catch {
    // No git history: say so with the build time.
  }
  return { sha: "", date: new Date().toISOString() };
}
