import type { DiagramSpec } from "./figure";

/**
 * The module graph: the root module importing feature modules and platform
 * modules. The root is the accent: the kernel walks the graph from it at
 * boot (the pulses).
 */
export const moduleGraph: DiagramSpec = {
  title: "The module graph",
  desc: "AppModule, the root, imports the feature modules and the platform modules. DealsModule registers DealService as a provider and DealsController as a controller, exports DealService, and imports CompaniesModule to use the CompanyService it exports. The platform modules (ConfigModule, DatabaseModule, HttpModule, AuthModule, QueueModule) are global: any module can use their exports without importing them, as DealsModule does with DatabaseModule. The kernel walks this graph from the root at boot and checks every edge before it builds anything.",
  legend: [{ kind: "dep", label: "imports or uses" }],
  wide: {
    width: 592,
    height: 432,
    groups: [
      { id: "features", x: 0, y: 112, w: 296, h: 320, label: "Feature modules", dashed: true },
      { id: "platform", x: 328, y: 112, w: 264, h: 320, label: "Platform modules · global", dashed: true },
    ],
    nodes: [
      { id: "app", x: 200, y: 0, w: 192, title: "AppModule", mono: true, sub: "the root · src/app.rs", icon: "module", tone: "accent" },
      {
        id: "deals",
        x: 16,
        y: 144,
        w: 264,
        h: 168,
        title: "DealsModule",
        mono: true,
        sub: "src/modules/deals",
        subMono: true,
        icon: "module",
        rows: [
          ["DealService", "provider"],
          ["DealsController", "controller"],
          ["DealService", "export"],
        ],
      },
      {
        id: "companies",
        x: 16,
        y: 352,
        w: 264,
        title: "CompaniesModule",
        mono: true,
        sub: "exports CompanyService",
        icon: "module",
      },
      { id: "config", x: 344, y: 160, w: 232, h: 40, title: "ConfigModule", mono: true },
      { id: "database", x: 344, y: 208, w: 232, h: 40, title: "DatabaseModule", mono: true },
      { id: "http", x: 344, y: 256, w: 232, h: 40, title: "HttpModule", mono: true },
      { id: "auth", x: 344, y: 304, w: 232, h: 40, title: "AuthModule", mono: true },
      { id: "queue", x: 344, y: 352, w: 232, h: 40, title: "QueueModule", mono: true },
    ],
    edges: [
      { from: "app.bottom", to: "features.top", kind: "dep", bend: 84, label: "imports", at: [304, 76], step: 0 },
      { from: "app.bottom", to: "platform.top", kind: "dep", bend: 84, step: 0 },
      { from: "deals.bottom", to: "companies.top", kind: "dep", label: "imports", at: [156, 336], step: 1 },
      { from: "deals.right", to: "database.left", kind: "dep", step: 1 },
    ],
  },
  narrow: {
    width: 272,
    height: 640,
    groups: [
      { id: "features", x: 24, y: 96, w: 240, h: 216, label: "Feature modules", dashed: true },
      { id: "platform", x: 24, y: 352, w: 240, h: 288, label: "Platform · global", dashed: true },
    ],
    nodes: [
      { id: "app", x: 48, y: 0, w: 192, title: "AppModule", mono: true, sub: "the root · src/app.rs", icon: "module", tone: "accent" },
      { id: "deals", x: 40, y: 136, w: 208, title: "DealsModule", mono: true, sub: "exports DealService", icon: "module" },
      {
        id: "companies",
        x: 40,
        y: 232,
        w: 208,
        title: "CompaniesModule",
        mono: true,
        sub: "exports CompanyService",
        icon: "module",
      },
      { id: "config", x: 40, y: 392, w: 208, h: 40, title: "ConfigModule", mono: true },
      { id: "database", x: 40, y: 440, w: 208, h: 40, title: "DatabaseModule", mono: true },
      { id: "http", x: 40, y: 488, w: 208, h: 40, title: "HttpModule", mono: true },
      { id: "auth", x: 40, y: 536, w: 208, h: 40, title: "AuthModule", mono: true },
      { id: "queue", x: 40, y: 584, w: 208, h: 40, title: "QueueModule", mono: true },
    ],
    edges: [
      { from: "app.bottom", to: "features.top", kind: "dep", label: "imports", at: [152, 80], step: 0 },
      { from: "app.left", to: "platform.left", kind: "dep", bend: 8, step: 0 },
      { from: "deals.bottom", to: "companies.top", kind: "dep", label: "imports", at: [152, 216], step: 1 },
      {
        from: "features.bottom",
        to: "platform.top",
        kind: "dep",
        label: "uses global exports",
        labelMono: false,
        at: [152, 336],
        step: 1,
      },
    ],
  },
};
