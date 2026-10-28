import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

/**
 * When a file last changed, for the sitemap and the docs' structured data:
 * the date of the last commit that touched it, or the file's modification
 * time where there is no git history (a tarball, a shallow CI checkout).
 * Paths are relative to the website directory (`content/docs/basics/validation.mdx`).
 */
const WEBSITE = process.cwd();

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

/** The last change to any of `files` (relative to the website directory), as an ISO date. */
export function lastModified(...files: string[]): string {
  const dates = files.map((file) => {
    const fromGit = commitDates().get(file.split(path.sep).join("/"));
    if (fromGit) return fromGit;
    const full = path.join(WEBSITE, file);
    return fs.existsSync(full) ? fs.statSync(full).mtime.toISOString() : new Date().toISOString();
  });
  return dates.reduce((a, b) => (new Date(a) > new Date(b) ? a : b));
}

/** The last change to anything under `dir` (relative to the website directory), as an ISO date. */
export function lastModifiedUnder(...dirs: string[]): string {
  const prefixes = dirs.map((d) => `${d.replace(/\/$/, "")}/`);
  let latest = "";
  for (const [file, date] of commitDates()) {
    if (prefixes.some((p) => file.startsWith(p) || `${file}/` === p) && (!latest || new Date(date) > new Date(latest))) latest = date;
  }
  return latest || new Date().toISOString();
}
