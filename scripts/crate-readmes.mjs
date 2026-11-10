#!/usr/bin/env node
// Writes the README.md and the two licence files of every published crate.
//
//   node scripts/crate-readmes.mjs           write them
//   node scripts/crate-readmes.mjs --check   write nothing; exit 1 if any differs
//
// Everything a README says comes from `cargo metadata` (name, description,
// Rust version, the `navree` facade's features) and from the table of guide
// pages below, so the files can be rewritten at any time. The `navree` crate
// itself uses the repository's README.md, which people see on crates.io.

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const check = process.argv.includes("--check");

// The guide page of each crate on navree.com. A crate with no page of its own
// points at the crate hierarchy, which says where it sits.
const GUIDE = {
  access: "security/authorization",
  ai: "ai/ai-sdk",
  api: "digging-deeper/openapi",
  audit: "security/audit-log",
  auth: "security/authentication",
  billing: "packages/billing",
  browser: "packages/browser-testing",
  cache: "digging-deeper/cache",
  cli: "digging-deeper/cli",
  config: "configuration/reference",
  crypto: "security/encryption-and-hashing",
  db: "database/getting-started",
  directory: "packages/directory-sync",
  dock: "tools/dock",
  documents: "packages/documents",
  domains: "packages/custom-domains",
  events: "digging-deeper/events",
  flags: "packages/feature-flags",
  fleet: "tools/fleet",
  graphql: "microservices/graphql",
  grpc: "microservices/grpc",
  harbor: "tools/harbor",
  helm: "tools/helm",
  http: "basics/routing",
  "http-client": "digging-deeper/http-client",
  i18n: "packages/localization",
  "i18n-macros": "packages/localization",
  integrations: "packages/integrations",
  kernel: "architecture/modules",
  lighthouse: "tools/lighthouse",
  log: "basics/logging",
  logbook: "tools/logbook",
  macros: "architecture/modules",
  mail: "digging-deeper/mail",
  markdown: "components/markdown",
  mcp: "ai/mcp-servers",
  media: "packages/media",
  micro: "microservices/introduction",
  mongo: "orm/mongodb",
  navigator: "ai/navigator",
  notify: "digging-deeper/notifications",
  oauth: "packages/oauth-server",
  observe: "digging-deeper/health-and-metrics",
  openapi: "digging-deeper/openapi",
  orm: "orm/getting-started",
  "orm-macros": "orm/getting-started",
  pdf: "packages/documents",
  phone: "basics/phone-numbers",
  queue: "digging-deeper/queues",
  realtime: "digging-deeper/broadcasting",
  redis: "digging-deeper/redis",
  schedule: "digging-deeper/scheduling",
  schema: "database/schema-builder",
  search: "packages/search",
  session: "basics/sessions-and-csrf",
  settings: "packages/settings",
  sheets: "packages/imports-and-exports",
  social: "packages/social-sign-in",
  sso: "security/single-sign-on",
  storage: "digging-deeper/storage",
  telemetry: "digging-deeper/telemetry",
  template: "digging-deeper/views",
  testing: "testing/getting-started",
  time: "digging-deeper/time-zones",
  ui: "ui/overview",
  "ui-macros": "ui/overview",
  validation: "basics/validation",
  "validation-macros": "basics/validation",
  view: "digging-deeper/views",
  webhooks: "packages/webhooks",
  workflows: "packages/workflows",
  workspace: "workspaces/introduction",
};
const FALLBACK_GUIDE = "architecture/crate-hierarchy";

// Crates that bundle third-party files say so in the README.
const NOTICE = {
  "navree-ui":
    "It bundles Lucide icons (ISC), flag-icons (MIT), the Geist font (OFL-1.1) and Unicode CLDR and libphonenumber data. Their notices are in `THIRD_PARTY.md`.",
  "navree-documents":
    "It bundles the Noto fonts (OFL-1.1); the licence text is in `fonts/OFL.txt`.",
  "navree-graphql":
    "It bundles GraphiQL and React (MIT); their licence texts are in `assets/`.",
  "navree-phone":
    "It holds libphonenumber's numbering plans (Apache-2.0); the notice is in `THIRD_PARTY.md`.",
};

// Crates whose README is written by hand.
const HAND_WRITTEN = new Set(["navree-ui"]);

function metadata() {
  const out = spawnSync("cargo", ["metadata", "--no-deps", "--format-version", "1", "--offline"], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
  });
  if (out.status !== 0) {
    console.error(out.stderr);
    process.exit(2);
  }
  return JSON.parse(out.stdout);
}

const meta = metadata();
const byName = new Map(meta.packages.map((p) => [p.name, p]));
const facade = byName.get("navree");
const published = meta.packages
  .filter((p) => p.name.startsWith("navree") && !(Array.isArray(p.publish) && p.publish.length === 0))
  .sort((a, b) => a.name.localeCompare(b.name));

// `pub use navree_cache as cache;` in the facade: crate -> module name.
const facadeLib = readFileSync(join(root, "crates/navree/src/lib.rs"), "utf8");
const moduleOf = new Map();
for (const m of facadeLib.matchAll(/^pub use (navree_[a-z0-9_]+) as ([a-z0-9_]+);/gm)) {
  moduleOf.set(m[1].replaceAll("_", "-"), m[2]);
}

const fullFeatures = new Set(facade.features.full ?? []);
const facadeDeps = new Map(facade.dependencies.map((d) => [d.name, d]));

// The facade feature that turns a crate on: the one that names `dep:<crate>`.
function facadeFeature(name) {
  const short = name.replace(/^navree-/, "");
  const names = Object.entries(facade.features)
    .filter(([, members]) => members.includes(`dep:${name}`))
    .map(([feature]) => feature);
  return names.includes(short) ? short : names[0];
}

// The published crates whose manifests depend on `name`.
function dependents(name) {
  return published.filter((p) => p.dependencies.some((d) => d.name === name && d.kind === null));
}

function guideUrl(name) {
  const slug = GUIDE[name.replace(/^navree-/, "")] ?? FALLBACK_GUIDE;
  return `https://navree.com/docs/${slug}/`;
}

// The sections every README ends with: how to turn the crate on, where its
// documentation is, and the Rust version and licence.
function sections(pkg) {
  const name = pkg.name;
  const lines = [];
  lines.push("## Use it", "");

  const dep = facadeDeps.get(name);
  const feature = facadeFeature(name);
  const mod = moduleOf.get(name);
  if (name === "navree-cli") {
    lines.push(
      "Install the `navree` command:",
      "",
      "```sh",
      "cargo install navree-cli --locked",
      "navree new my-app",
      "```",
      "",
      "Building it compiles Dock, Harbor and Navigator, which takes a few minutes. The release page also has prebuilt binaries for Linux, macOS and Windows.",
    );
  } else if (dep && !dep.optional) {
    lines.push(
      `It is always part of \`navree\`, the framework crate${mod ? `, as \`navree::${mod}\`` : ""}:`,
      "",
      "```sh",
      "cargo add navree",
      "```",
      "",
      `You can also depend on it alone with \`cargo add ${name}\`.`,
    );
  } else if (feature) {
    const onByDefault = fullFeatures.has(feature);
    lines.push(
      onByDefault
        ? `It is part of \`navree\`'s default \`full\` feature set${mod ? `, as \`navree::${mod}\`` : ""}:`
        : `It is opt-in. Turn on the \`${feature}\` feature of \`navree\`${mod ? ` to get \`navree::${mod}\`` : ""}:`,
      "",
      "```sh",
      onByDefault ? "cargo add navree" : `cargo add navree --features ${feature}`,
      "```",
      "",
      `You can also depend on it alone with \`cargo add ${name}\`.`,
    );
  } else {
    const parents = dependents(name).filter((p) => p.name !== "navree-cli");
    const parent = parents.length === 1 ? parents[0].name : null;
    if (parent) {
      const parentFeature = facadeFeature(parent);
      lines.push(
        `Its macros are re-exported by \`${parent}\`${parentFeature ? `, and so by \`navree\` with the \`${parentFeature}\` feature` : ""}. Depend on that crate instead of this one.`,
        "",
        "```sh",
        parentFeature && !fullFeatures.has(parentFeature)
          ? `cargo add navree --features ${parentFeature}`
          : "cargo add navree",
        "```",
      );
    } else {
      lines.push("Add it to your project with `cargo add " + name + "`.");
    }
  }
  lines.push("");

  lines.push(
    "## Documentation",
    "",
    `- [The guide on navree.com](${guideUrl(name)})`,
    `- [API reference on docs.rs](https://docs.rs/${name})`,
    `- [Source and issues on GitHub](https://github.com/NavreeHQ/navree/tree/main/crates/${name})`,
    "",
  );

  lines.push("## Rust version and licence", "");
  lines.push(`This crate needs Rust ${pkg.rust_version} or later.`, "");
  const licence = pkg.license;
  lines.push(
    licence === "MIT OR Apache-2.0"
      ? "Licensed under either of the MIT licence ([LICENSE-MIT](LICENSE-MIT)) or the Apache licence, version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option."
      : `Navree's own code is licensed under either of the MIT licence ([LICENSE-MIT](LICENSE-MIT)) or the Apache licence, version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option. The licence expression of this crate is \`${licence}\`.`,
  );
  if (NOTICE[name]) lines.push("", NOTICE[name]);
  lines.push("");
  return lines.join("\n");
}

// Markdown would read `Valid<T>` as a tag; code-like words go in backticks.
function codeSpans(text) {
  return text.replace(/#\[[^\]]*\]|\b\w+<\w+>|\b[a-z]+\.toml\b/g, (m) => `\`${m}\``);
}

function readme(pkg) {
  return [
    `# ${pkg.name}`,
    "",
    codeSpans(pkg.description),
    "",
    "Part of [Navree](https://navree.com), the complete application framework for Rust.",
    "",
    sections(pkg),
  ].join("\n");
}

// A README written by hand keeps its text; the sections sit between two
// markers at its end.
const START = "<!-- crate-readme:start -->";
const END = "<!-- crate-readme:end -->";
function handWritten(pkg, current) {
  const block = `${START}\n${sections(pkg)}${END}\n`;
  const text = current.replace(/\s+$/, "");
  const at = text.indexOf(START);
  const head = at === -1 ? text : text.slice(0, at).replace(/\s+$/, "");
  return `${head}\n\n${block}`;
}

let wrong = 0;
function put(file, text) {
  const current = existsSync(file) ? readFileSync(file, "utf8") : null;
  if (current === text) return;
  wrong += 1;
  if (check) {
    console.error(`differs: ${file.slice(root.length + 1)}`);
  } else {
    writeFileSync(file, text);
  }
}

const mit = readFileSync(join(root, "LICENSE-MIT"), "utf8");
const apache = readFileSync(join(root, "LICENSE-APACHE"), "utf8");

for (const pkg of published) {
  const dir = dirname(pkg.manifest_path);
  const file = join(dir, "README.md");
  if (HAND_WRITTEN.has(pkg.name)) put(file, handWritten(pkg, readFileSync(file, "utf8")));
  else if (pkg.name !== "navree") put(file, readme(pkg));
  put(join(dir, "LICENSE-MIT"), mit);
  put(join(dir, "LICENSE-APACHE"), apache);
}

if (check && wrong > 0) {
  console.error(`${wrong} file(s) differ: run node scripts/crate-readmes.mjs`);
  process.exit(1);
}
console.log(check ? "crate READMEs and licence files are current" : `wrote ${wrong} file(s) for ${published.length} crates`);
