#!/usr/bin/env node
// Fails when a published npm package lacks what npm needs, or when the private
// one is not private.
//
//   node scripts/check-packaging.mjs          read the package.json files and the README and licence files
//   node scripts/check-packaging.mjs --pack   also run `pnpm -r pack` (after `pnpm build`) and read the tarballs

import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const jsRoot = resolve(here, "..");
const repoRoot = resolve(jsRoot, "../..");
const PUBLISHED = ["client", "bff", "next", "nuxt", "theme"];
const PRIVATE = ["deepmerge"];
const REPOSITORY = "git+https://github.com/NavreeHQ/navree.git";

const problems = [];
const fail = (message) => problems.push(message);
const read = (file) => readFileSync(file, "utf8").replaceAll("\r\n", "\n");

const licences = {
  "LICENSE-MIT": read(join(repoRoot, "LICENSE-MIT")),
  "LICENSE-APACHE": read(join(repoRoot, "LICENSE-APACHE")),
};

for (const dir of PUBLISHED) {
  const folder = join(jsRoot, dir);
  const pkg = JSON.parse(read(join(folder, "package.json")));
  const name = pkg.name ?? `@navree/${dir}`;
  const need = (ok, message) => {
    if (!ok) fail(`${name}: ${message}`);
  };

  need(name === `@navree/${dir}`, `the name is @navree/${dir}`);
  need(pkg.private !== true, "is private, so it cannot be published");
  need(typeof pkg.description === "string" && pkg.description.length > 20, "has no description");
  need(pkg.license === "MIT OR Apache-2.0", 'license is "MIT OR Apache-2.0"');
  need(
    pkg.repository?.type === "git" &&
      pkg.repository?.url === REPOSITORY &&
      pkg.repository?.directory === `packages/js/${dir}`,
    `repository is { type: "git", url: "${REPOSITORY}", directory: "packages/js/${dir}" }`,
  );
  need(
    typeof pkg.homepage === "string" && pkg.homepage.startsWith("https://navree.com/"),
    "homepage is a navree.com address",
  );
  need(
    pkg.bugs?.url === "https://github.com/NavreeHQ/navree/issues",
    "bugs.url is the repository's issues page",
  );
  need(
    Array.isArray(pkg.keywords) && pkg.keywords.length > 0 && pkg.keywords.every((k) => typeof k === "string"),
    "has keywords",
  );
  need(typeof pkg.engines?.node === "string", "names the Node.js versions in engines.node");
  need(
    pkg.publishConfig?.access === "public" && pkg.publishConfig?.provenance === true,
    'publishConfig is { access: "public", provenance: true }',
  );
  for (const file of ["LICENSE-MIT", "LICENSE-APACHE"]) {
    need(pkg.files?.includes(file), `files lists ${file} (npm does not add a licence file with a suffix)`);
    const path = join(folder, file);
    need(existsSync(path) && read(path) === licences[file], `${file} is missing or differs from the root copy`);
  }
  const readme = join(folder, "README.md");
  need(existsSync(readme), "has no README.md");
  if (existsSync(readme)) {
    const text = read(readme);
    need(text.startsWith(`# ${name}`), "the README starts with the package name");
    need(text.includes(`npm install ${name}`), "the README says how to install it");
    need(text.includes("https://navree.com/docs/"), "the README links the guide");
  }
  for (const [dependency, range] of Object.entries(pkg.dependencies ?? {})) {
    if (dependency.startsWith("@navree/")) {
      need(range === "workspace:*", `${dependency} is "workspace:*"; publishing turns it into the exact version`);
    }
  }
}

for (const dir of PRIVATE) {
  const pkg = JSON.parse(read(join(jsRoot, dir, "package.json")));
  if (pkg.private !== true) fail(`${pkg.name}: must be "private": true`);
}

if (process.argv.includes("--pack")) {
  const out = mkdtempSync(join(tmpdir(), "navree-pack-"));
  const packed = spawnSync("pnpm", ["-r", "pack", "--pack-destination", out], {
    cwd: jsRoot,
    encoding: "utf8",
    shell: process.platform === "win32",
  });
  if (packed.status !== 0) {
    fail(`pnpm -r pack failed:\n${packed.stdout}${packed.stderr}`);
  } else {
    for (const dir of PUBLISHED) {
      const tarball = `navree-${dir}-1.0.0.tgz`;
      const listed = spawnSync("tar", ["-tzf", tarball], { cwd: out, encoding: "utf8" });
      if (listed.status !== 0) {
        fail(`@navree/${dir}: cannot read ${tarball} (${listed.stderr})`);
        continue;
      }
      const files = listed.stdout.split("\n").filter(Boolean);
      for (const file of ["package/package.json", "package/README.md", "package/LICENSE-MIT", "package/LICENSE-APACHE"]) {
        if (!files.includes(file)) fail(`@navree/${dir}: the tarball lacks ${file}`);
      }
      if (!files.some((file) => file.startsWith("package/dist/"))) {
        fail(`@navree/${dir}: the tarball has no dist/ (run pnpm build first)`);
      }
      const stray = files.filter((file) => /\.test\.|\/test\/|\/tests\//.test(file));
      if (stray.length > 0) fail(`@navree/${dir}: the tarball holds test files: ${stray.join(", ")}`);
    }
  }
}

if (problems.length > 0) {
  console.error(problems.map((p) => `- ${p}`).join("\n"));
  process.exit(1);
}
console.log("npm packages are ready to publish");
