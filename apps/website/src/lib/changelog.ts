import { execFileSync } from "node:child_process";

import { site } from "../../site.config.ts";

/**
 * The changelog: one entry per release, newest first. Navree 1.0 is the
 * first release, so it is the only entry; later releases add theirs here.
 */
export type Change = {
  /** Full commit id. */
  hash: string;
  /** `feat` or `fix`. */
  type: "feat" | "fix";
  /** The part in parentheses, or "". */
  scope: string;
  /** The subject after `type(scope): `, cut to a readable length. */
  subject: string;
  /** The commit's date with its offset, ISO 8601. */
  date: string;
  /** The commit's day, `2026-11-03`, in the committer's time zone. */
  day: string;
  /** True for a breaking change (`feat!:`). */
  breaking: boolean;
};

export type Day = { day: string; changes: Change[]; updated: string };

/** Scopes of apps and crates that have left the repository; none so far. */
export const RETIRED_SCOPES: readonly string[] = [];

/** Every release, newest first. */
const RELEASES: Change[] = [
  {
    hash: "124d1fa8b90db30def87d0e1dd4c6d57c43a89f7",
    type: "feat",
    scope: "",
    subject: "Navree 1.0, the first release.",
    date: "2026-11-03T19:31:00+05:30",
    day: "2026-11-03",
    breaking: false,
  },
];

/** Every release's entries, newest first. */
export function changes(): Change[] {
  return RELEASES;
}

/** The changes grouped by day, newest day first. */
export function changelog(): Day[] {
  const days = new Map<string, Change[]>();
  for (const change of changes()) days.set(change.day, [...(days.get(change.day) ?? []), change]);
  return [...days]
    .sort(([a], [b]) => (a < b ? 1 : -1))
    .map(([day, list]) => ({ day, changes: list, updated: list.reduce((a, b) => (a.date > b.date ? a : b)).date }));
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
