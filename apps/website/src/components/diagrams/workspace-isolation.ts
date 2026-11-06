import type { DiagramSpec } from "./figure";

/**
 * The three ways to keep a workspace's data apart. Wide: each column is one
 * strategy, from the setting that picks it down to where the data lives and
 * what keeps it apart; the scoped unit of work (the accent) is what every
 * strategy hangs from. Narrow: one node per strategy.
 */
export const workspaceIsolation: DiagramSpec = {
  title: "Three ways to isolate a workspace's data",
  desc: "The request's scoped unit of work follows workspace.isolation, which picks one of three strategies. Row, the default: every workspace's rows live in the same tables with a workspace_id column, and WorkspaceRepo scopes each query, with row-level security on PostgreSQL as well. Schema, PostgreSQL only: each workspace has a schema of its own tables, workspace_ followed by its id, and the transaction's search_path points at it. Database: each workspace has a database of its own, and WorkspaceDatabases opens one connection pool for each on first use.",
  legend: [{ kind: "flow", label: "workspace.isolation" }],
  wide: {
    width: 592,
    height: 376,
    nodes: [
      { id: "uow", x: 176, y: 0, w: 240, title: "Scoped unit of work", sub: "workspace.isolation", subMono: true, icon: "layers", tone: "accent" },
      { id: "row", x: 0, y: 112, w: 176, title: "Row", sub: "shared tables", icon: "queue" },
      { id: "schema", x: 208, y: 112, w: 176, title: "Schema", sub: "PostgreSQL only", icon: "layers" },
      { id: "database", x: 416, y: 112, w: 176, title: "Database", sub: "one per workspace", icon: "database" },
      { id: "rowdata", x: 0, y: 216, w: 176, title: "deals", mono: true, sub: "every workspace's rows" },
      { id: "schemadata", x: 208, y: 216, w: 176, title: "workspace_<id>", mono: true, sub: "its own tables" },
      { id: "dbdata", x: 416, y: 216, w: 176, title: "workspace_<id>", mono: true, sub: "its own database" },
      { id: "rowguard", x: 0, y: 320, w: 176, title: "WorkspaceRepo", mono: true, sub: "+ RLS on PostgreSQL" },
      { id: "schemaguard", x: 208, y: 320, w: 176, title: "search_path", mono: true, sub: "set for the transaction" },
      { id: "dbguard", x: 416, y: 320, w: 176, title: "WorkspaceDatabases", mono: true, sub: "a pool per workspace" },
    ],
    edges: [
      { from: "uow.bottom", to: "row.top", bend: 84, step: 0 },
      { from: "uow.bottom", to: "schema.top", bend: 84, step: 0 },
      { from: "uow.bottom", to: "database.top", bend: 84, step: 0 },
      { from: "row.bottom", to: "rowdata.top", label: "lives in", labelMono: false, at: [96, 198], step: 1 },
      { from: "schema.bottom", to: "schemadata.top", label: "lives in", labelMono: false, at: [304, 198], step: 1 },
      { from: "database.bottom", to: "dbdata.top", label: "lives in", labelMono: false, at: [512, 198], step: 1 },
      { from: "rowdata.bottom", to: "rowguard.top", label: "kept apart by", labelMono: false, at: [96, 302], step: 2 },
      { from: "schemadata.bottom", to: "schemaguard.top", label: "kept apart by", labelMono: false, at: [304, 302], step: 2 },
      { from: "dbdata.bottom", to: "dbguard.top", label: "kept apart by", labelMono: false, at: [512, 302], step: 2 },
    ],
  },
  narrow: {
    width: 272,
    height: 400,
    nodes: [
      { id: "uow", x: 24, y: 0, w: 248, title: "Scoped unit of work", sub: "workspace.isolation", subMono: true, icon: "layers", tone: "accent" },
      { id: "row", x: 24, y: 104, w: 248, title: "Row", sub: "shared tables · WorkspaceRepo", icon: "queue" },
      { id: "schema", x: 24, y: 208, w: 248, title: "Schema", sub: "a schema each · search_path", icon: "layers" },
      { id: "database", x: 24, y: 312, w: 248, title: "Database", sub: "a database and a pool each", icon: "database" },
    ],
    edges: [
      { from: "uow.left", to: "row.left", bend: 8, step: 0 },
      { from: "uow.left", to: "schema.left", bend: 8, step: 0 },
      { from: "uow.left", to: "database.left", bend: 8, step: 0 },
    ],
  },
};
