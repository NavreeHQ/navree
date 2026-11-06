// @vitest-environment node
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { navreeOptions } from "../../navree.options";

// MOD-012
const root = fileURLToPath(new URL("../../", import.meta.url));
const repoRoot = join(root, "../..");
const read = (file: string) => readFileSync(join(root, file), "utf8");

/** Every file under `dir` (relative to the app), without dependencies and build output. */
function filesUnder(dir: string): string[] {
  const start = join(root, dir);
  if (!existsSync(start)) return [];
  const found: string[] = [];
  const walk = (current: string) => {
    for (const name of readdirSync(current)) {
      if (name === "node_modules" || name === ".nuxt" || name === ".output") continue;
      const path = join(current, name);
      if (statSync(path).isDirectory()) walk(path);
      else found.push(relative(root, path).replaceAll("\\", "/"));
    }
  };
  walk(start);
  return found;
}

/** The files under `dir` whose text matches `pattern`. */
function filesMatching(dir: string, pattern: RegExp): string[] {
  return filesUnder(dir).filter((file) => pattern.test(read(file)));
}

describe("the Navree module runs the backend-for-frontend", () => {
  it("is registered in nuxt.config.ts, with the CRM's settings from navree.options.ts", () => {
    const config = read("nuxt.config.ts");
    expect(config).toMatch(/modules:\s*\[[^\]]*"@navree\/nuxt"[^\]]*\]/);
    expect(config).toMatch(/navree:\s*navreeOptions\(\)/);
  });

  it("turns the session gate and the socket on, and the abilities", () => {
    const options = navreeOptions({});
    expect(options.apiUrl).toBe("http://127.0.0.1:8092");
    expect(options.sessionGate).toEqual({ loginPath: "/login" });
    expect(options.realtime).toEqual({ key: "navree-crm-key", wsHost: "127.0.0.1", wsPort: 8092 });
    expect(options.abilities).toBe(true);
  });

  it("leaves the gate and the socket off with the mock API, which has no server session", () => {
    for (const mocks of ["1", "true"]) {
      const options = navreeOptions({ NUXT_PUBLIC_API_MOCKS: mocks });
      expect(options.sessionGate).toBe(false);
      expect(options.realtime).toBe(false);
      expect(options.abilities).toBe(true);
    }
    expect(navreeOptions({ NUXT_PUBLIC_API_MOCKS: "0" }).sessionGate).toEqual({ loginPath: "/login" });
  });

  it("depends on @navree/nuxt and @navree/client, linked from the checkout", () => {
    const manifest = JSON.parse(read("package.json")) as {
      dependencies: Record<string, string>;
      devDependencies: Record<string, string>;
    };
    expect(manifest.dependencies["@navree/nuxt"]).toBe("link:../../packages/js/nuxt");
    expect(manifest.dependencies["@navree/client"]).toBe("link:../../packages/js/client");
    for (const name of ["nuxt", "client"]) {
      const linked = JSON.parse(readFileSync(join(repoRoot, "packages/js", name, "package.json"), "utf8")) as { name: string };
      expect(linked.name).toBe(`@navree/${name}`);
    }
    // h3 was only for the app's own Nitro handlers.
    expect(manifest.devDependencies.h3).toBeUndefined();
  });

  it("records the links in the lock file", () => {
    const lock = read("pnpm-lock.yaml");
    for (const name of ["nuxt", "client"]) {
      const link = `link:../../packages/js/${name}`;
      expect(lock).toContain(`'@navree/${name}':\n        specifier: ${link}\n        version: ${link}`);
    }
  });

  it("keeps the override that gives Nitro's bundler @navree/deepmerge", () => {
    expect(read("pnpm-workspace.yaml")).toMatch(/^ {2}deepmerge: link:\.\.\/\.\.\/packages\/js\/deepmerge$/m);
    expect(read("pnpm-lock.yaml")).toMatch(/^ {2}deepmerge: link:\.\.\/\.\.\/packages\/js\/deepmerge$/m);
  });

  it("lets Vite use one copy of Vue, vue-router, CASL and pusher-js, and serve the linked packages", () => {
    const config = read("nuxt.config.ts");
    for (const name of ["vue", "vue-router", "@casl/ability", "@casl/vue", "pusher-js"]) {
      expect(config).toMatch(new RegExp(`dedupe:\\s*\\[[^\\]]*"${name}"`));
    }
    expect(config).toMatch(/fs:\s*\{\s*allow:/);
  });

  it("lets Nitro bundle the linked packages, with Nitro's own h3", () => {
    const config = read("nuxt.config.ts");
    for (const name of ["@navree/nuxt", "@navree/bff", "@navree/client"]) {
      expect(config).toMatch(new RegExp(`inline:\\s*\\[[^\\]]*"${name}"`));
    }
    expect(config).toMatch(/alias:\s*\{\s*h3\s*\}/);
  });
});

describe("the app carries no copy of the packages' code", () => {
  it("has no BFF, token refresh or session gate of its own in server/", () => {
    expect(filesUnder("server")).toEqual([]);
    expect(filesMatching("server", /createBff|refresh_token|navree_at|navree_rt|navree_2fa|set-cookie|Bearer/i)).toEqual([]);
  });

  it("has no copy of the typed client, the error class, the pusher wrapper or the CASL plugin", () => {
    for (const file of [
      "app/lib/api/fetch.ts",
      "app/lib/api/errors.ts",
      "app/lib/realtime/pusher.ts",
      "app/lib/realtime/connection.ts",
      "app/plugins/03.casl.ts",
    ]) {
      expect(existsSync(join(root, file)), file).toBe(false);
    }
  });

  it("reaches the API only through @navree/client, never with its own fetch core or error class", () => {
    expect(filesMatching("app", /lib\/api\/(fetch|errors)["']|\bApiError\b|\bisApiError\b|\.rawFetch\(/)).toEqual([]);
  });

  it("opens no socket and builds no ability itself", () => {
    expect(filesMatching("app", /from ["']pusher-js["']|new Pusher\(|createMongoAbility|abilitiesPlugin/)).toEqual([]);
  });

  it("maps validation errors through the module's applyFieldErrors, and waits for the rules before drawing the shell", () => {
    expect(read("app/lib/api/form-errors.ts")).toContain('from "@navree/nuxt/vue"');
    expect(read("app/layouts/default.vue")).toContain("useAbilityReady()");
  });

  it("signs in, listens and checks permissions through the module", () => {
    expect(read("app/pages/login.vue")).toContain("useNavreeAuth");
    expect(read("app/composables/useRealtime.ts")).toContain('from "@navree/nuxt/realtime"');
    expect(read("app/features/auth/queries.ts")).toContain("useNavreeAuth");
    expect(read("app/middleware/session.global.ts")).toContain("currentUserLoader");
    expect(read("app/lib/casl/ability.ts")).not.toMatch(/createMongoAbility|export function create/);
  });
});
