import type { DiagramSpec } from "./figure";

/**
 * A workspace request: resolve the workspace, check membership, scope the
 * unit of work, then the isolation strategy decides where the rows live.
 * The scoped unit of work is the accent: it is what keeps every query
 * inside one workspace.
 */
export const multiTenancy: DiagramSpec = {
  title: "Multi-tenancy by workspace",
  desc: "A request names its workspace, for example with the header X-Workspace: acme. Navree resolves the workspace from the header, the path, the subdomain or a token claim, and answers 400 or 404 when it can't. It then checks that the user is a member, and answers 403 to anyone else; the member's role adds its permissions. The request's unit of work is scoped to the workspace by setting navree.workspace on its transaction. The configured isolation decides where the data lives: rows with a workspace_id column guarded by PostgreSQL row-level security, a schema per workspace, or a database per workspace.",
  legend: [{ kind: "flow", label: "request" }],
  wide: {
    width: 592,
    height: 504,
    nodes: [
      { id: "req", x: 0, y: 0, w: 352, title: "Request", sub: "X-Workspace: acme", subMono: true, icon: "request" },
      { id: "resolve", x: 0, y: 104, w: 352, title: "Resolve the workspace", sub: "header · path · subdomain · token claim", icon: "funnel" },
      { id: "member", x: 0, y: 208, w: 352, title: "Check membership", sub: "the member's role adds its permissions", icon: "users" },
      {
        id: "uow",
        x: 0,
        y: 312,
        w: 352,
        title: "Scoped unit of work",
        sub: "set_config('navree.workspace', id)",
        subMono: true,
        icon: "layers",
        tone: "accent",
      },
      { id: "e404", x: 416, y: 104, w: 176, title: "400 · 404", mono: true, sub: "none named, or unknown", tone: "muted" },
      { id: "e403", x: 416, y: 208, w: 176, title: "403", mono: true, sub: "not a member", tone: "muted" },
      { id: "row", x: 0, y: 448, w: 176, title: "Row", sub: "row-level security", icon: "queue" },
      { id: "schema", x: 208, y: 448, w: 176, title: "Schema", sub: "one per workspace", icon: "layers" },
      { id: "database", x: 416, y: 448, w: 176, title: "Database", sub: "one per workspace", icon: "database" },
    ],
    edges: [
      { from: "req.bottom", to: "resolve.top", step: 0 },
      { from: "resolve.bottom", to: "member.top", step: 1 },
      { from: "member.bottom", to: "uow.top", step: 2 },
      { from: "resolve.right", to: "e404.left" },
      { from: "member.right", to: "e403.left" },
      { from: "uow.bottom", to: "row.top", bend: 408, label: "workspace.isolation", at: [184, 392], step: 3 },
      { from: "uow.bottom", to: "schema.top", bend: 408, step: 3 },
      { from: "uow.bottom", to: "database.top", bend: 408, step: 3 },
    ],
  },
  narrow: {
    width: 272,
    height: 640,
    nodes: [
      { id: "req", x: 24, y: 0, w: 248, title: "Request", sub: "X-Workspace: acme", subMono: true, icon: "request" },
      { id: "resolve", x: 24, y: 96, w: 248, title: "Resolve the workspace", sub: "400 or 404 if it can't", icon: "funnel" },
      { id: "member", x: 24, y: 192, w: 248, title: "Check membership", sub: "403 for anyone else", icon: "users" },
      {
        id: "uow",
        x: 24,
        y: 288,
        w: 248,
        title: "Scoped unit of work",
        sub: "sets navree.workspace",
        icon: "layers",
        tone: "accent",
      },
      { id: "row", x: 24, y: 424, w: 248, title: "Row", sub: "row-level security", icon: "queue" },
      { id: "schema", x: 24, y: 504, w: 248, title: "Schema", sub: "one per workspace", icon: "layers" },
      { id: "database", x: 24, y: 584, w: 248, title: "Database", sub: "one per workspace", icon: "database" },
    ],
    edges: [
      { from: "req.bottom", to: "resolve.top", step: 0 },
      { from: "resolve.bottom", to: "member.top", step: 1 },
      { from: "member.bottom", to: "uow.top", step: 2 },
      { from: "uow.left", to: "row.left", bend: 8, step: 3 },
      { from: "uow.left", to: "schema.left", bend: 8, step: 3 },
      { from: "uow.left", to: "database.left", bend: 8, step: 3 },
    ],
    notes: [{ x: 24, y: 404, text: "workspace.isolation", mono: true }],
  },
};
