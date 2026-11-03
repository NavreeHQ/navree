/**
 * Facts about the reference CRM, read from its source at build time, so the
 * showcase follows the code. Server components only.
 */
import fs from "node:fs";
import path from "node:path";

/** The CRM API in the repository (`apps/crm-api`), from `apps/website`. */
const CRM_API = path.resolve(process.cwd(), "..", "crm-api");

/**
 * How many Navree framework modules crm-core's `platform()` configures: its
 * `.module(XModule::for_root(..))` and `.module(XModule::from_config())`
 * calls. The CRM's own modules are registered without a constructor, so they
 * aren't counted. A moved file fails the build rather than show a stale number.
 */
export function frameworkModuleCount(): number {
  const source = fs.readFileSync(path.join(CRM_API, "crates", "crm-core", "src", "platform.rs"), "utf8");
  const start = source.indexOf("pub fn platform(");
  const body = source.slice(start, source.indexOf("\n}", start));
  const count = [...body.matchAll(/\.module\(\w+Module::(?:for_root|from_config)\(/g)].length;
  if (start < 0 || count === 0) throw new Error("crm-source: no framework modules found in crm-core's platform()");
  return count;
}
