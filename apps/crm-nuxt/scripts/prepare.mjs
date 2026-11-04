// Runs after `pnpm install`: `nuxt prepare` writes the typed .nuxt folder. The @navree/nuxt module
// is loaded from the built packages (packages/js/*/dist), so this waits until they are built.
//
//   pnpm packages    # install and build packages/js once, and again after changing it
//   pnpm install     # then this app
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";

const built = ["client", "bff", "nuxt"].every((name) =>
  existsSync(fileURLToPath(new URL(`../../../packages/js/${name}/dist`, import.meta.url))),
);

if (!built) {
  console.warn("[crm-nuxt] The Navree packages are not built yet: run `pnpm packages`, then `pnpm install` again.");
  process.exit(0);
}

const result = spawnSync("nuxt", ["prepare"], { stdio: "inherit", shell: true });
process.exit(result.status ?? 1);
