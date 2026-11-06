import fs from "node:fs";
import path from "node:path";

/**
 * The tools every app's Navigator has, read from the source that defines them
 * (`tools()` in `crates/navree-navigator/src/tools.rs`), so a page that says how
 * many there are, or lists them, follows the code. Server only: it reads the
 * repository with `node:fs`.
 */
const SOURCE = path.resolve(process.cwd(), "..", "..", "crates", "navree-navigator", "src", "tools.rs");

let cache: string[] | null = null;

/** The names of the tools, in the order the source gives them. A source that has changed shape fails the build. */
export function navigatorToolNames(): string[] {
  if (cache) return cache;
  const source = fs.readFileSync(SOURCE, "utf8");
  const start = source.indexOf("pub fn tools() -> Vec<ToolDef>");
  if (start < 0) throw new Error("navigator-tools: crates/navree-navigator/src/tools.rs has no `pub fn tools() -> Vec<ToolDef>`");
  // The function ends where the next item starts: the doc comment of `Navigator`.
  const end = source.indexOf("\n/// Answers the tools for one app.", start);
  const body = source.slice(start, end < 0 ? undefined : end);
  const names = [...body.matchAll(/\bname: "([a-z_]+)"/g)].map((m) => m[1]);
  if (names.length === 0 || new Set(names).size !== names.length) {
    throw new Error("navigator-tools: could not read distinct tool names from crates/navree-navigator/src/tools.rs");
  }
  cache = names;
  return names;
}

/** How many tools every app's Navigator has. */
export function navigatorToolCount(): number {
  return navigatorToolNames().length;
}
