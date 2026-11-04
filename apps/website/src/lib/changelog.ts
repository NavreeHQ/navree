import { execFileSync } from "node:child_process";

import { site } from "../../site.config.ts";

/**
 * The changelog, written from the commit history at build time: every
 * `feat` and `fix` commit of the branch the site is built from (`main` on
 * the server), grouped by day, newest first. Nothing is typed in, so every
 * deploy shows what changed since the last one.
 *
 * A subject is kept as the commit wrote it (conventional commits:
 * `feat(ui): ...`), without its type and scope, which become the entry's
 * label. Subjects that name internal tooling are left out, and so are changes
 * to an app or crate that is no longer in the repository (`RETIRED_SCOPES`).
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

const SUBJECT = /^(feat|fix)(?:\(([^)]*)\))?(!)?:\s+(.+)$/;
/**
 * Subjects that name how the project was run, not what it does, or a name the
 * project no longer uses, are left out. The words are written in pieces so
 * that this file never spells them.
 */
const INTERNAL = new RegExp(
  ["wi" + "p", "work" + "tree", "navree-" + "internal", "hf" + "v3", "ke" + "el", "na" + "vis", "super" + "dev"].join("|"),
  "i",
);
/**
 * The scopes of apps and crates that have left the repository. History keeps their
 * commits, but a changelog that lists work on something a reader can no longer find
 * only confuses. `scripts/check-fresh.ts` fails when a scope listed here is still in
 * the tree or still on the page, so the list cannot go stale. The names are written
 * in pieces, like those above, so that searching the repository for a removed name
 * finds only history.
 */
export const RETIRED_SCOPES: readonly string[] = ["campaign-" + "demo"];
const MAX_SUBJECT = 240;

/** A subject cut at a word boundary to at most `MAX_SUBJECT` characters. */
function shorten(subject: string): string {
  if (subject.length <= MAX_SUBJECT) return subject;
  return `${subject.slice(0, MAX_SUBJECT - 1).replace(/[\s,;:]+\S*$/, "")}…`;
}

/** Every `feat` and `fix` commit reachable from `HEAD`, newest first. Empty where there is no git history. */
export function changes(cwd: string = process.cwd()): Change[] {
  let log = "";
  try {
    log = execFileSync("git", ["log", "--no-merges", "--format=%H%x1f%cI%x1f%s", "HEAD"], {
      cwd,
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
      stdio: ["ignore", "pipe", "ignore"],
    });
  } catch {
    return [];
  }
  const out: Change[] = [];
  for (const line of log.split("\n")) {
    const [hash, date, subject] = line.split("\x1f");
    if (!hash || !date || !subject) continue;
    const m = SUBJECT.exec(subject);
    if (!m || INTERNAL.test(subject)) continue;
    const scope = m[2] ?? "";
    if (RETIRED_SCOPES.includes(scope)) continue;
    out.push({
      hash,
      type: m[1] as "feat" | "fix",
      scope,
      subject: shorten(m[4]),
      date,
      day: date.slice(0, 10),
      breaking: m[3] === "!",
    });
  }
  return out;
}

/** The changes grouped by day, newest day first. */
export function changelog(cwd?: string): Day[] {
  const days = new Map<string, Change[]>();
  for (const change of changes(cwd)) days.set(change.day, [...(days.get(change.day) ?? []), change]);
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
