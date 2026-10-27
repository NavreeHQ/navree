// @vitest-environment node
import { describe, expect, it, vi } from "vitest";
import { setupNavree, type NavreeModuleOptions, type NuxtKit } from "./setup.js";

function fakeNuxt() {
  return {
    options: {
      runtimeConfig: { public: {} as Record<string, unknown> } as Record<string, unknown> & { public: Record<string, unknown> },
      build: { transpile: [] as unknown[] },
    },
  };
}

function fakeKit(): NuxtKit & { [K in keyof NuxtKit]: ReturnType<typeof vi.fn> } {
  return {
    resolve: vi.fn((path: string) => `/pkg/dist/${path.replace(/^\.\//, "")}`),
    addServerHandler: vi.fn(),
    addServerImports: vi.fn(),
    addImports: vi.fn(),
    addPlugin: vi.fn(),
    addComponent: vi.fn(),
  } as never;
}

const run = (options: NavreeModuleOptions = {}) => {
  const nuxt = fakeNuxt();
  const kit = fakeKit();
  setupNavree(options, nuxt, kit);
  return { nuxt, kit };
};

describe("the Nuxt module", () => {
  it("mounts the BFF on /api/** and /broadcasting/auth, and the session gate", () => {
    const { kit } = run();
    const routes = kit.addServerHandler.mock.calls.map(([h]) => h);
    expect(routes).toEqual([
      { route: "/api/**", handler: "/pkg/dist/runtime/server/bff-handler.js" },
      { route: "/broadcasting/auth", method: "post", handler: "/pkg/dist/runtime/server/bff-handler.js" },
      { middleware: true, handler: "/pkg/dist/runtime/server/session-gate.js" },
    ]);
  });

  it("mounts extra prefixes (a server-rendered tool at /admin) and can leave the gate out", () => {
    const { kit } = run({ mounts: ["/admin"], sessionGate: false, broadcasting: false });
    expect(kit.addServerHandler.mock.calls.map(([h]) => (h as { route?: string }).route)).toEqual(["/api/**", "/admin/**"]);
  });

  it("puts the settings in runtime config, so NUXT_NAVREE_* variables override them", () => {
    const { nuxt } = run({ apiUrl: "http://127.0.0.1:8092", sessionGate: { loginPath: "/sign-in", publicPaths: ["/"] } });
    expect(nuxt.options.runtimeConfig.navree).toEqual({
      apiUrl: "http://127.0.0.1:8092",
      workspaceHeader: "X-Workspace",
      sessionGate: { loginPath: "/sign-in", publicPaths: ["/"] },
    });
    expect(nuxt.options.runtimeConfig.public.navree).toEqual({
      authPrefix: "/api/auth",
      operationsPath: "/api/operations",
      loginPath: "/sign-in",
      workspaceHeader: "X-Workspace",
      realtime: false,
      abilities: false,
    });
  });

  it("auto-imports the composables and registers the data table", () => {
    const { kit, nuxt } = run();
    const names = kit.addImports.mock.calls.flatMap(([imports]) => (imports as { name: string }[]).map((i) => i.name));
    expect(names).toEqual(
      expect.arrayContaining(["useNavree", "useNavreeAuth", "useWorkspace", "useNavreeClient", "useOperation", "useListQuery", "applyFieldErrors"]),
    );
    expect(names).not.toContain("useChannel");
    expect(names).not.toContain("useAbility");
    expect(kit.addComponent.mock.calls.map(([c]) => (c as { name: string }).name)).toEqual(["NavreeDataTable", "NavreePagination"]);
    expect(kit.addServerImports.mock.calls[0]![0]).toEqual([
      { name: "getNavreeSession", from: "/pkg/dist/runtime/server/utils.js" },
      { name: "useNavreeBff", from: "/pkg/dist/runtime/server/utils.js" },
    ]);
    expect(kit.addPlugin.mock.calls.map(([p]) => p)).toEqual(["/pkg/dist/runtime/plugin.js"]);
    expect(nuxt.options.build.transpile).toContain("/pkg/dist/runtime");
  });

  it("adds real time and abilities only when asked, so their packages stay optional", () => {
    const realtime = { key: "app-key", wsHost: "127.0.0.1", wsPort: 8080 };
    const { kit, nuxt } = run({ realtime, abilities: true });
    const names = kit.addImports.mock.calls.flatMap(([imports]) => (imports as { name: string }[]).map((i) => i.name));
    expect(names).toEqual(expect.arrayContaining(["useChannel", "useRealtimeState", "useAbility"]));
    expect(kit.addPlugin.mock.calls.map(([p]) => p)).toEqual([
      "/pkg/dist/runtime/plugin.js",
      { src: "/pkg/dist/runtime/plugin-realtime.js", mode: "client" },
      "/pkg/dist/runtime/plugin-abilities.js",
    ]);
    expect(kit.addComponent.mock.calls.map(([c]) => (c as { name: string }).name)).toContain("Can");
    expect((nuxt.options.runtimeConfig.public.navree as { realtime: unknown }).realtime).toEqual(realtime);
  });

  it("keeps settings the app already put in runtime config", () => {
    const nuxt = fakeNuxt();
    nuxt.options.runtimeConfig.navree = { apiUrl: "http://api:9000" };
    setupNavree({ apiUrl: "http://127.0.0.1:8080" }, nuxt, fakeKit());
    expect((nuxt.options.runtimeConfig.navree as { apiUrl: string }).apiUrl).toBe("http://api:9000");
  });
});
