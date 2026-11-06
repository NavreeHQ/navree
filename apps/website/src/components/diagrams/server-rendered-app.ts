import type { DiagramSpec } from "./figure";

/**
 * A server-rendered app. Wide: one binary holds the pages (the accent), the
 * JSON routes for other clients and the live hub, and all three sit on the
 * same feature modules and one database; the hub tells an open page to fetch
 * again. Narrow: the same parts as a column.
 */
export const serverRenderedApp: DiagramSpec = {
  title: "A server-rendered app",
  desc: "One binary serves the browser its pages. A page handler returns Navree UI components, and the browser gets HTML with its stylesheet and only the scripts the page uses. The same binary can serve JSON routes to other clients, such as mobile apps and partners. Pages and routes call the same feature modules, which own the services, the access rules and the models, and which reach the one database in a unit of work for each request. When a module commits a change, the live hub tells the open pages that show it, and each page fetches the part that changed.",
  legend: [
    { kind: "flow", label: "a request" },
    { kind: "async", label: "a live update" },
  ],
  wide: {
    width: 592,
    height: 392,
    groups: [{ id: "binary", x: 0, y: 96, w: 592, h: 224, label: "One binary" }],
    nodes: [
      { id: "browser", x: 208, y: 0, w: 176, title: "Browser", sub: "pages and forms", icon: "browser" },
      { id: "api", x: 16, y: 136, w: 176, title: "API routes", sub: "for other clients", icon: "route" },
      { id: "pages", x: 208, y: 136, w: 176, title: "Pages", sub: "typed components", icon: "code", tone: "accent" },
      { id: "live", x: 400, y: 136, w: 176, title: "Live hub", sub: "server-sent events", icon: "broadcast" },
      {
        id: "modules",
        x: 16,
        y: 232,
        w: 560,
        title: "Feature modules",
        sub: "services, access rules and models, shared by pages and API",
        icon: "module",
      },
      { id: "db", x: 208, y: 352, w: 176, h: 40, title: "Database", icon: "database" },
    ],
    edges: [
      { from: "browser.bottom", to: "pages.top", step: 0, label: "HTML", at: [304, 84] },
      { from: "pages.bottom", to: "modules.top", step: 1 },
      { from: "api.bottom", to: "modules.top", bend: 212, step: 1 },
      { from: "modules.bottom", to: "db.top", step: 2 },
      { from: "modules.top", to: "live.bottom", kind: "async", bend: 212 },
      { from: "live.top", to: "browser.right", kind: "async", label: "a nudge", labelMono: false, at: [496, 84] },
    ],
  },
  narrow: {
    width: 272,
    height: 552,
    groups: [{ id: "binary", x: 0, y: 88, w: 272, h: 376, label: "One binary" }],
    nodes: [
      { id: "browser", x: 16, y: 0, w: 240, title: "Browser", sub: "pages and forms", icon: "browser" },
      { id: "pages", x: 16, y: 128, w: 240, title: "Pages", sub: "typed components", icon: "code", tone: "accent" },
      { id: "api", x: 16, y: 216, w: 240, title: "API routes", sub: "for other clients", icon: "route" },
      { id: "modules", x: 16, y: 304, w: 240, title: "Feature modules", sub: "services, access, models", icon: "module" },
      { id: "live", x: 16, y: 392, w: 240, title: "Live hub", sub: "tells open pages", icon: "broadcast" },
      { id: "db", x: 16, y: 496, w: 240, h: 40, title: "Database", icon: "database" },
    ],
    edges: [
      { from: "browser.bottom", to: "pages.top", step: 0 },
      { from: "pages.left", to: "modules.left", bend: 8, step: 1 },
      { from: "api.bottom", to: "modules.top", step: 1 },
      { from: "modules.right", to: "db.right", bend: 264, step: 2 },
      { from: "modules.bottom", to: "live.top", kind: "async" },
    ],
  },
};
