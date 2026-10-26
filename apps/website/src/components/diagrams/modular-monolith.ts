import type { DiagramSpec } from "./figure";

/**
 * One binary, modules with imports and exports inside the DI container,
 * one database. The container is the accent: it is what checks the
 * boundaries at boot.
 */
export const modularMonolith: DiagramSpec = {
  title: "A modular monolith",
  desc: "Clients send HTTP requests to one binary. Inside it the DI container holds every module and checks the graph when the app boots. HttpModule routes each request to a feature module: CompaniesModule, DealsModule or ReportsModule. DealsModule imports CompaniesModule and ReportsModule imports DealsModule, so each uses only what the other exports. The feature modules reach the one PostgreSQL database through DatabaseModule, in one transaction per request.",
  legend: [
    { kind: "flow", label: "request" },
    { kind: "dep", label: "imports" },
  ],
  wide: {
    width: 592,
    height: 552,
    groups: [
      { id: "binary", x: 0, y: 96, w: 592, h: 360, label: "One binary" },
      { id: "container", x: 16, y: 136, w: 560, h: 304, label: "DI container · checked at boot", tone: "accent" },
    ],
    nodes: [
      { id: "clients", x: 208, y: 0, w: 176, title: "Clients", sub: "web · mobile · agents", icon: "browser" },
      { id: "http", x: 208, y: 176, w: 176, title: "HttpModule", mono: true, sub: "router · middleware" },
      { id: "companies", x: 24, y: 272, w: 160, title: "CompaniesModule", mono: true, sub: "exports CompanyService" },
      { id: "deals", x: 216, y: 272, w: 160, title: "DealsModule", mono: true, sub: "exports DealService" },
      { id: "reports", x: 408, y: 272, w: 160, title: "ReportsModule", mono: true, sub: "exports nothing" },
      { id: "db", x: 208, y: 368, w: 176, title: "DatabaseModule", mono: true, sub: "pool · unit of work" },
      { id: "pg", x: 208, y: 496, w: 176, title: "PostgreSQL", sub: "one database", icon: "database" },
    ],
    edges: [
      { from: "clients.bottom", to: "http.top", label: "HTTP", at: [304, 80], step: 0 },
      { from: "http.bottom", to: "companies.top", step: 1 },
      { from: "http.bottom", to: "deals.top", step: 1 },
      { from: "http.bottom", to: "reports.top", step: 1 },
      { from: "deals.left", to: "companies.right", kind: "dep" },
      { from: "reports.left", to: "deals.right", kind: "dep" },
      { from: "companies.bottom", to: "db.top", step: 2 },
      { from: "deals.bottom", to: "db.top", step: 2 },
      { from: "reports.bottom", to: "db.top", step: 2 },
      {
        from: "db.bottom",
        to: "pg.top",
        label: "one transaction per request",
        labelMono: false,
        at: [304, 480],
        step: 3,
      },
    ],
  },
  narrow: {
    width: 272,
    height: 712,
    groups: [
      { id: "binary", x: 0, y: 96, w: 272, h: 520, label: "One binary" },
      { id: "container", x: 8, y: 136, w: 256, h: 464, label: "DI container", tone: "accent" },
    ],
    nodes: [
      { id: "clients", x: 48, y: 0, w: 176, title: "Clients", sub: "web · mobile · agents", icon: "browser" },
      { id: "http", x: 48, y: 176, w: 176, title: "HttpModule", mono: true, sub: "router · middleware" },
      { id: "companies", x: 40, y: 264, w: 192, title: "CompaniesModule", mono: true, sub: "exports CompanyService" },
      { id: "deals", x: 40, y: 352, w: 192, title: "DealsModule", mono: true, sub: "exports DealService" },
      { id: "reports", x: 40, y: 440, w: 192, title: "ReportsModule", mono: true, sub: "exports nothing" },
      { id: "db", x: 48, y: 528, w: 176, title: "DatabaseModule", mono: true, sub: "pool · unit of work" },
      { id: "pg", x: 48, y: 656, w: 176, title: "PostgreSQL", sub: "one database", icon: "database" },
    ],
    edges: [
      { from: "clients.bottom", to: "http.top", label: "HTTP", at: [144, 80], step: 0 },
      { from: "http.left", to: "companies.left", bend: 24, step: 1 },
      { from: "http.left", to: "deals.left", bend: 24, step: 1 },
      { from: "http.left", to: "reports.left", bend: 24, step: 1 },
      { from: "deals.top", to: "companies.bottom", kind: "dep" },
      { from: "reports.top", to: "deals.bottom", kind: "dep" },
      { from: "companies.right", to: "db.right", bend: 248, step: 2 },
      { from: "deals.right", to: "db.right", bend: 248, step: 2 },
      { from: "reports.right", to: "db.right", bend: 248, step: 2 },
      { from: "db.bottom", to: "pg.top", label: "one transaction", labelMono: false, at: [144, 640], step: 3 },
    ],
  },
};
