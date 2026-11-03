// Generates the toolkit's icons and flags from pinned releases.
//
//   node icons.mjs            fetch the pinned sources, check them against icons.lock.json,
//                             and write src/icons/lucide.rs, src/flags/data.rs and the
//                             icon tags for the docs
//   node icons.mjs --check    do the same in memory and fail when a file on disk differs
//   node icons.mjs pin <source> <version>
//                             pin another release of one source (lucide, flags,
//                             cldr-territories or cldr-subdivisions); the registry's
//                             checksum is recorded, then run the generator again
//
// The sources are npm tarballs. Each is refused unless its SHA-512 is the one
// in icons.lock.json, before any file inside it is read. The generated files
// are committed, so building the crate never needs Node or the network.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { packageFiles, registryPin, sha256 } from "./icons/fetch.mjs";
import { flagsRust, readFlags } from "./icons/flags.mjs";
import { lucideRust, readLucide } from "./icons/lucide.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const crate = resolve(here, "..");
const lockPath = join(crate, "icons.lock.json");
const lock = JSON.parse(readFileSync(lockPath, "utf8"));

const PACKAGES = {
  lucide: "lucide-static",
  flags: "flag-icons",
  "cldr-territories": "cldr-localenames-full",
  "cldr-subdivisions": "cldr-subdivisions-full",
};

const args = process.argv.slice(2);

if (args[0] === "pin") {
  const [, source, version] = args;
  if (!(source in PACKAGES) || !version) {
    console.error(`usage: node icons.mjs pin <${Object.keys(PACKAGES).join("|")}> <version>`);
    process.exit(2);
  }
  lock.sources[source] = await registryPin(PACKAGES[source], version);
  writeFileSync(lockPath, `${JSON.stringify(lock, null, 2)}\n`);
  console.log(`${source} pinned to ${PACKAGES[source]}@${version}. Run: node icons.mjs`);
  process.exit(0);
}

const check = args.includes("--check");
if (args.some((arg) => arg !== "--check")) {
  console.error("usage: node icons.mjs [--check | pin <source> <version>]");
  process.exit(2);
}

const sources = {};
for (const [name, pin] of Object.entries(lock.sources)) sources[name] = await packageFiles(pin);

const lucide = readLucide(sources.lucide);
const flags = readFlags(sources.flags, sources["cldr-territories"], sources["cldr-subdivisions"]);

if (lucide.version !== lock.sources.lucide.version) throw new Error("the lucide package's version is not the pin's");
if (flags.version !== lock.sources.flags.version) throw new Error("the flag-icons package's version is not the pin's");

const tagsJson = `{\n${Object.entries(lucide.tags)
  .map(([name, tags]) => `  ${JSON.stringify(name)}: ${JSON.stringify(tags)}`)
  .join(",\n")}\n}\n`;

/** Output path (from the crate's root) and its text. */
const outputs = {
  "src/icons/lucide.rs": lucideRust(lucide),
  "src/flags/data.rs": flagsRust(flags),
  "../../apps/ui-docs/data/lucide-tags.json": tagsJson,
};

let stale = 0;
const hashes = {};
for (const [path, text] of Object.entries(outputs)) {
  const full = join(crate, path);
  hashes[path] = sha256(text);
  if (check) {
    const onDisk = existsSync(full) ? readFileSync(full, "utf8").replace(/\r\n/g, "\n") : null;
    if (onDisk !== text) {
      console.error(`${path}: differs from a fresh generation`);
      stale += 1;
    }
    if (lock.outputs?.[path] !== hashes[path]) {
      console.error(`${path}: icons.lock.json records another checksum`);
      stale += 1;
    }
  } else {
    mkdirSync(dirname(full), { recursive: true });
    writeFileSync(full, text);
  }
}

if (check) {
  if (stale) {
    console.error("Run: node crates/navree-ui/runtime/icons.mjs");
    process.exit(1);
  }
  console.log("icons and flags are current");
} else {
  lock.outputs = hashes;
  writeFileSync(lockPath, `${JSON.stringify(lock, null, 2)}\n`);
  const kb = (text) => `${(Buffer.byteLength(text) / 1024).toFixed(0)} KB`;
  console.log(
    `lucide ${lucide.version}: ${lucide.icons.length} icons, ${lucide.renamed.length} old names (${kb(outputs["src/icons/lucide.rs"])})\n` +
      `flag-icons ${flags.version}: ${flags.flags.length} flags (${kb(outputs["src/flags/data.rs"])}); ` +
      `names: ${flags.fromCldr.length} from CLDR ${flags.cldrVersion}, ${flags.fromFlagIcons.length} from flag-icons (${flags.fromFlagIcons.join(", ")})`,
  );
}
