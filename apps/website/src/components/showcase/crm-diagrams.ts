import type { DiagramSpec } from "@/components/diagrams";

/**
 * The CRM API's structure: one binary, four feature crates that depend
 * only on crm-core, and the framework under it. crm-core is the accent: it
 * is what keeps the features apart.
 */
export const crmCrates: DiagramSpec = {
  title: "The CRM API",
  desc: "crm-api is one binary. Its app.rs registers the CRM's modules on top of the platform. They live in four feature crates: crm-users (users and roles), crm-companies (companies, contacts and the CSV export), crm-deals (deals, the board and deal events) and crm-activities (tasks, the dashboard and reminders). Each feature crate depends only on crm-core, never on another feature; crm-core holds the models, the migrations, the roles and the abilities function. crm-core's platform() configures 19 Navree framework modules, from HTTP and the database to auth, access control, queues and real time.",
  legend: [{ kind: "dep", label: "depends on" }],
  wide: {
    width: 592,
    height: 496,
    groups: [{ id: "features", x: 0, y: 104, w: 592, h: 184, label: "Feature crates" }],
    nodes: [
      { id: "api", x: 200, y: 0, w: 192, title: "crm-api", mono: true, sub: "one binary · src/app.rs", icon: "server" },
      { id: "users", x: 16, y: 144, w: 272, title: "crm-users", mono: true, sub: "users and roles, for admins", icon: "module" },
      { id: "companies", x: 304, y: 144, w: 272, title: "crm-companies", mono: true, sub: "companies, contacts, CSV export", icon: "module" },
      { id: "deals", x: 16, y: 216, w: 272, title: "crm-deals", mono: true, sub: "deals, the board, events", icon: "module" },
      { id: "activities", x: 304, y: 216, w: 272, title: "crm-activities", mono: true, sub: "tasks, the dashboard, reminders", icon: "module" },
      {
        id: "core",
        x: 152,
        y: 336,
        w: 288,
        title: "crm-core",
        mono: true,
        sub: "entities · roles · abilities",
        icon: "layers",
        tone: "accent",
      },
      { id: "navree", x: 152, y: 440, w: 288, title: "navree", mono: true, sub: "19 framework modules", icon: "module" },
    ],
    edges: [
      { from: "api.bottom", to: "features.top", kind: "dep", label: "registers their modules", labelMono: false, at: [304, 86] },
      { from: "features.bottom", to: "core.top", kind: "dep", label: "each only on crm-core", labelMono: false, at: [304, 318] },
      { from: "core.bottom", to: "navree.top", kind: "dep", label: "platform()", at: [304, 422] },
    ],
  },
  narrow: {
    width: 272,
    height: 640,
    groups: [{ id: "features", x: 0, y: 104, w: 272, h: 328, label: "Feature crates" }],
    nodes: [
      { id: "api", x: 40, y: 0, w: 192, title: "crm-api", mono: true, sub: "one binary", icon: "server" },
      { id: "users", x: 8, y: 144, w: 256, title: "crm-users", mono: true, sub: "users and roles, for admins", icon: "module" },
      { id: "companies", x: 8, y: 216, w: 256, title: "crm-companies", mono: true, sub: "companies, contacts, CSV", icon: "module" },
      { id: "deals", x: 8, y: 288, w: 256, title: "crm-deals", mono: true, sub: "deals, the board, events", icon: "module" },
      { id: "activities", x: 8, y: 360, w: 256, title: "crm-activities", mono: true, sub: "tasks, dashboard, reminders", icon: "module" },
      {
        id: "core",
        x: 16,
        y: 480,
        w: 240,
        title: "crm-core",
        mono: true,
        sub: "entities · roles · abilities",
        icon: "layers",
        tone: "accent",
      },
      { id: "navree", x: 16, y: 584, w: 240, title: "navree", mono: true, sub: "19 framework modules", icon: "module" },
    ],
    edges: [
      { from: "api.bottom", to: "features.top", kind: "dep", label: "app.rs", at: [144, 86] },
      { from: "features.bottom", to: "core.top", kind: "dep", label: "only on crm-core", labelMono: false, at: [144, 462] },
      { from: "core.bottom", to: "navree.top", kind: "dep", label: "platform()", at: [144, 566] },
    ],
  },
};

/**
 * One API, two front ends: the Next.js and the Nuxt app each run a BFF with
 * the same contract. The contract is the accent.
 */
export const crmFrontEnds: DiagramSpec = {
  title: "One API, two front ends",
  desc: "The browser calls only its own origin and holds no token. Either front end runs a backend-for-frontend with the same contract: the Next.js app in a catch-all route handler (app/api/[...path]/route.ts), the Nuxt app in a Nitro server route (server/api/[...path].ts). Both keep the access and refresh tokens in httpOnly cookies, check the origin of every write, refresh once per refresh token, and call the same Navree API, crm-api, with Authorization: Bearer.",
  legend: [{ kind: "flow", label: "request" }],
  wide: {
    width: 592,
    height: 344,
    groups: [{ id: "contract", x: 16, y: 104, w: 560, h: 120, label: "Same contract", tone: "accent" }],
    nodes: [
      { id: "browser", x: 200, y: 0, w: 192, title: "Browser", sub: "no tokens", icon: "browser" },
      { id: "next", x: 40, y: 144, w: 240, title: "Next.js BFF", sub: "app/api/[...path]/route.ts", subMono: true },
      { id: "nuxt", x: 312, y: 144, w: 240, title: "Nuxt BFF", sub: "server/api/[...path].ts", subMono: true },
      { id: "api", x: 200, y: 288, w: 192, title: "Navree API", sub: "crm-api", subMono: true, icon: "server" },
    ],
    edges: [
      { from: "browser.bottom", to: "next.top", bend: 80, step: 0 },
      { from: "browser.bottom", to: "nuxt.top", bend: 80, step: 0, label: "same origin", labelMono: false, at: [304, 72] },
      { from: "next.bottom", to: "api.top", bend: 256, step: 1 },
      { from: "nuxt.bottom", to: "api.top", bend: 256, step: 1, label: "Authorization: Bearer", at: [304, 278] },
    ],
  },
  narrow: {
    width: 272,
    height: 408,
    groups: [{ id: "contract", x: 0, y: 104, w: 272, h: 192, label: "Same contract", tone: "accent" }],
    nodes: [
      { id: "browser", x: 40, y: 0, w: 192, title: "Browser", sub: "no tokens", icon: "browser" },
      { id: "next", x: 16, y: 144, w: 240, title: "Next.js BFF", sub: "app/api/[...path]/route.ts", subMono: true },
      { id: "nuxt", x: 16, y: 216, w: 240, title: "Nuxt BFF", sub: "server/api/[...path].ts", subMono: true },
      { id: "api", x: 40, y: 352, w: 192, title: "Navree API", sub: "crm-api", subMono: true, icon: "server" },
    ],
    edges: [
      { from: "browser.bottom", to: "contract.top", label: "same origin", labelMono: false, at: [144, 86], step: 0 },
      { from: "contract.bottom", to: "api.top", label: "Bearer", at: [144, 330], step: 1 },
    ],
  },
};
