import type { DiagramSpec } from "./figure";

/**
 * API-first: the contract comes from the code. The OpenAPI document is the
 * accent: it is generated from the DTOs and controllers, and every client
 * is generated from it.
 */
export const apiFirst: DiagramSpec = {
  title: "API-first",
  desc: "Validated DTOs (garde rules) feed versioned controllers under /api/v1. Navree generates an OpenAPI 3.1 document from both when the app boots and serves it at /api/openapi.json. openapi:export writes the document to a file, and typed clients are generated from it: the web app in TypeScript, mobile apps in Swift or Kotlin, partners and agents in any language. Validation failures (400) and refusals from controllers (401, 403, 404) share one JSON error shape: statusCode, message and error.",
  legend: [{ kind: "flow", label: "generated from" }],
  wide: {
    width: 592,
    height: 288,
    nodes: [
      { id: "dto", x: 0, y: 0, w: 160, title: "Validated DTOs", sub: "garde rules", icon: "braces" },
      { id: "ctrl", x: 216, y: 0, w: 160, title: "Controllers", sub: "versioned: /api/v1", icon: "route" },
      { id: "doc", x: 432, y: 0, w: 160, title: "OpenAPI 3.1", sub: "built at boot", icon: "document", tone: "accent" },
      {
        id: "err",
        x: 0,
        y: 112,
        w: 376,
        title: "One error shape",
        sub: '{ "statusCode", "message", "error" }',
        subMono: true,
        icon: "braces",
      },
      { id: "web", x: 0, y: 232, w: 176, title: "Web app", sub: "TypeScript", icon: "browser" },
      { id: "mobile", x: 208, y: 232, w: 176, title: "Mobile apps", sub: "Swift · Kotlin", icon: "phone" },
      { id: "partners", x: 416, y: 232, w: 176, title: "Partners, agents", sub: "any language", icon: "terminal" },
    ],
    edges: [
      { from: "dto.right", to: "ctrl.left", label: "input", at: [188, 20], anchor: "middle", step: 0 },
      { from: "ctrl.right", to: "doc.left", label: "#[api]", at: [404, 20], anchor: "middle", step: 1 },
      { from: "dto.bottom", to: "err.top", bend: 84, label: "400", at: [88, 76] },
      { from: "ctrl.bottom", to: "err.top", bend: 84, label: "401 · 403 · 404", at: [304, 76] },
      { from: "doc.bottom", to: "web.top", bend: 200, label: "openapi:export", at: [504, 132], anchor: "end", step: 2 },
      { from: "doc.bottom", to: "mobile.top", bend: 200, step: 2 },
      { from: "doc.bottom", to: "partners.top", bend: 200, step: 2 },
    ],
  },
  narrow: {
    width: 272,
    height: 600,
    nodes: [
      { id: "dto", x: 24, y: 0, w: 216, title: "Validated DTOs", sub: "garde rules", icon: "braces" },
      { id: "ctrl", x: 24, y: 96, w: 216, title: "Controllers", sub: "versioned: /api/v1", icon: "route" },
      { id: "doc", x: 24, y: 192, w: 216, title: "OpenAPI 3.1", sub: "built at boot", icon: "document", tone: "accent" },
      { id: "err", x: 24, y: 288, w: 216, title: "One error shape", sub: "statusCode, message, error", subMono: true },
      { id: "web", x: 24, y: 400, w: 248, title: "Web app", sub: "TypeScript", icon: "browser" },
      { id: "mobile", x: 24, y: 472, w: 248, title: "Mobile apps", sub: "Swift · Kotlin", icon: "phone" },
      { id: "partners", x: 24, y: 544, w: 248, title: "Partners, agents", sub: "any language", icon: "terminal" },
    ],
    edges: [
      { from: "dto.bottom", to: "ctrl.top", label: "input", at: [140, 80], step: 0 },
      { from: "ctrl.bottom", to: "doc.top", label: "#[api]", at: [140, 176], step: 1 },
      { from: "dto.right", to: "err.right", bend: 256 },
      { from: "ctrl.right", to: "err.right", bend: 256 },
      { from: "doc.left", to: "web.left", bend: 8, step: 2 },
      { from: "doc.left", to: "mobile.left", bend: 8, step: 2 },
      { from: "doc.left", to: "partners.left", bend: 8, step: 2 },
    ],
    notes: [{ x: 24, y: 380, text: "openapi:export", mono: true }],
  },
};
