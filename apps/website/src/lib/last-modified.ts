import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

/**
 * When a file last changed, for the sitemap and the docs' structured data:
 * the date of the last commit that touched it, or the file's modification
 * time where there is no git history (a tarball, a shallow CI checkout).
 *
 * Paths are relative to the website directory (`content/docs/basics/validation.mdx`).
 * A page can also depend on a file outside the website: the proof a rule on
 * `/architecture/` names, the constant an example app's page reads. Such a path
 * starts with `../` (`../../crates/navree-ui/tests/budgets.rs`) and is dated
 * from the repository the same way.
 */
const WEBSITE = process.cwd();
/** The repository's root, from `apps/website`. */
const REPO = path.resolve(WEBSITE, "..", "..");

let commits: Map<string, string> | null = null;

/** One `git log` over the website: path (relative to the website) → ISO date of its last commit. */
function commitDates(): Map<string, string> {
  if (commits) return commits;
  commits = new Map();
  try {
    const log = execFileSync("git", ["log", "--format=%x00%cI", "--name-only", "--relative", "--", "."], {
      cwd: WEBSITE,
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
      stdio: ["ignore", "pipe", "ignore"],
    });
    let date = "";
    for (const line of log.split("\n")) {
      if (line.startsWith("\0")) date = line.slice(1).trim();
      else if (line.trim() && date && !commits.has(line.trim())) commits.set(line.trim(), date);
    }
  } catch {
    // No git: fall back to modification times below.
  }
  return commits;
}

/** Whether a source path is outside the website directory. */
const isOutside = (source: string) => source.split(path.sep).join("/").startsWith("../");

/** The newest modification time of a file, or of anything under a directory. */
function newestMtime(full: string): string | null {
  let stat: fs.Stats;
  try {
    stat = fs.statSync(full);
  } catch {
    return null;
  }
  if (!stat.isDirectory()) return stat.mtime.toISOString();
  let newest = stat.mtime.toISOString();
  for (const entry of fs.readdirSync(full)) {
    const child = newestMtime(path.join(full, entry));
    if (child && child > newest) newest = child;
  }
  return newest;
}

const outsideDates = new Map<string, string | null>();

/** The date of the last commit that touched a path outside the website (a file or a directory), or its modification time; null if the path is not there. */
function outsideDate(source: string): string | null {
  const full = path.resolve(WEBSITE, source);
  if (outsideDates.has(full)) return outsideDates.get(full) ?? null;
  let date: string | null = null;
  try {
    date = execFileSync("git", ["log", "-1", "--format=%cI", "--", full], { cwd: REPO, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim() || null;
  } catch {
    // No git: fall back to modification times below.
  }
  date ??= newestMtime(full);
  outsideDates.set(full, date);
  return date;
}

/** Every path of `sources` that is outside the website and is not there: a page that names a file that has moved should be told. */
export function missingOutsideSources(sources: readonly string[]): string[] {
  return sources.filter((source) => isOutside(source) && !fs.existsSync(path.resolve(WEBSITE, source)));
}

const latestOf = (dates: string[]) => dates.reduce((a, b) => (new Date(a) > new Date(b) ? a : b));

/** The last change to any of `files` (relative to the website directory, or outside it with `../`), as an ISO date. */
export function lastModified(...files: string[]): string {
  const dates = files.map((file) => {
    if (isOutside(file)) return outsideDate(file) ?? new Date().toISOString();
    const fromGit = commitDates().get(file.split(path.sep).join("/"));
    if (fromGit) return fromGit;
    const full = path.join(WEBSITE, file);
    return fs.existsSync(full) ? fs.statSync(full).mtime.toISOString() : new Date().toISOString();
  });
  return latestOf(dates);
}

/** The last change to anything under `dirs` (relative to the website directory, or outside it with `../`), as an ISO date. */
export function lastModifiedUnder(...dirs: string[]): string {
  const inside = dirs.filter((dir) => !isOutside(dir));
  const prefixes = inside.map((d) => `${d.replace(/\/$/, "")}/`);
  let latest = "";
  for (const [file, date] of commitDates()) {
    if (prefixes.some((p) => file.startsWith(p) || `${file}/` === p) && (!latest || new Date(date) > new Date(latest))) latest = date;
  }
  for (const dir of dirs.filter(isOutside)) {
    const date = outsideDate(dir);
    if (date && (!latest || new Date(date) > new Date(latest))) latest = date;
  }
  return latest || new Date().toISOString();
}

/** The date of the newest commit in the repository (the changelog changes with every one), or now where there is no git history. */
export function latestCommit(): string {
  try {
    const date = execFileSync("git", ["log", "-1", "--format=%cI"], {
      cwd: WEBSITE,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
    if (date) return date;
  } catch {
    // No git: fall through.
  }
  return new Date().toISOString();
}
