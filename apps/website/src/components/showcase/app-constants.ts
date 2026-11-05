/**
 * A number an example app keeps in a constant of its own source, read when the
 * site is built, so a page about the app says what the app does. Server only.
 */
import fs from "node:fs";
import path from "node:path";

/** The repository's root, from `apps/website`. */
const REPO = path.resolve(process.cwd(), "..", "..");

/** The number `pub const NAME: usize = 20_000;` (or any integer type) holds in `file`, from the repository's root. A missing constant fails the build. */
export function appConstant(file: string, name: string): number {
  const source = fs.readFileSync(path.join(REPO, file), "utf8");
  const found = new RegExp(String.raw`pub const ${name}: (?:[iu](?:8|16|32|64|128)|[iu]size) = ([\d_]+);`).exec(source);
  if (!found) throw new Error(`${file} has no integer constant ${name}`);
  return Number(found[1].replace(/_/g, ""));
}
