import fs from "node:fs";
import path from "node:path";

/**
 * The version of Navree this site describes: `[workspace.package] version`
 * in the repository's `Cargo.toml`, the same one `scripts/agent-files.ts`
 * names the docs bundle after. Read once at build time, so the structured
 * data, `llms.txt` and the pages never say a different number.
 */
let cached: string | null = null;

export function navreeVersion(root: string = process.cwd()): string {
  if (cached) return cached;
  const toml = fs.readFileSync(path.join(root, "..", "..", "Cargo.toml"), "utf8");
  const version = /\[workspace\.package\][^[]*?\bversion\s*=\s*"([^"]+)"/.exec(toml)?.[1];
  if (!version) throw new Error("version: no [workspace.package] version in the repository's Cargo.toml");
  cached = version;
  return version;
}
