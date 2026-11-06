import type { DiagramSpec } from "./figure";

/**
 * How the parts of the ORM fit together. Wide: the left column is a query,
 * from the model through the builder and the unit of work (the accent) to
 * the database and back as records; a save runs the other way, along the
 * left margin, and its events and hooks sit beside the unit of work.
 * Narrow: the same story as one column.
 */
export const ormParts: DiagramSpec = {
  title: "How the ORM fits together",
  desc: "A model is a struct with #[derive(Model)] and one typed column constant per field. Model::query() returns a builder that collects typed conditions, relations to load and scopes. Terminal methods such as get take an executor: the request's unit of work, or the database pool, which joins the current unit of work when there is one. The unit of work sends the statements, written for the database in use, to PostgreSQL, MySQL, SQLite or MongoDB. What comes back are records and collections of records: your model plus dirty tracking and loaded relations. Saving a record goes back through the executor and runs the model's events, observers and hooks. Changes recorded in the unit of work reach the audit trail and broadcasts once it commits.",
  legend: [
    { kind: "flow", label: "a query" },
    { kind: "async", label: "a save" },
  ],
  wide: {
    width: 592,
    height: 408,
    nodes: [
      { id: "model", x: 24, y: 0, w: 328, title: "Model", sub: "a struct, a typed column constant per field", icon: "module" },
      { id: "builder", x: 24, y: 88, w: 328, title: "Builder", sub: "Deal::query(): conditions, relations, scopes", icon: "funnel" },
      { id: "uow", x: 24, y: 176, w: 328, title: "Unit of work or pool", sub: "the request's transaction; &Db joins it", icon: "layers", tone: "accent" },
      { id: "db", x: 24, y: 264, w: 328, title: "Database", sub: "PostgreSQL · MySQL · SQLite · MongoDB", icon: "database" },
      { id: "records", x: 24, y: 352, w: 328, title: "Records", sub: "Record<M> tracks changes and loaded relations", icon: "braces" },
      { id: "events", x: 416, y: 176, w: 176, title: "Events and hooks", sub: "observers, listeners", icon: "event" },
      { id: "after", x: 416, y: 264, w: 176, title: "After the commit", sub: "audit, broadcasts", icon: "broadcast" },
    ],
    edges: [
      { from: "model.bottom", to: "builder.top", step: 0 },
      { from: "builder.bottom", to: "uow.top", step: 1 },
      { from: "uow.bottom", to: "db.top", step: 2 },
      { from: "db.bottom", to: "records.top", step: 3 },
      {
        from: "records.left",
        to: "uow.left",
        kind: "async",
        via: [
          [8, 380],
          [8, 204],
        ],
      },
      { from: "uow.right", to: "events.left", kind: "async" },
      { from: "events.bottom", to: "after.top", kind: "async" },
    ],
  },
  narrow: {
    width: 272,
    height: 584,
    nodes: [
      { id: "model", x: 24, y: 0, w: 248, title: "Model", sub: "a typed column per field", icon: "module" },
      { id: "builder", x: 24, y: 88, w: 248, title: "Builder", sub: "conditions, relations, scopes", icon: "funnel" },
      { id: "uow", x: 24, y: 176, w: 248, title: "Unit of work or pool", sub: "the request's transaction", icon: "layers", tone: "accent" },
      { id: "db", x: 24, y: 264, w: 248, title: "Database", sub: "PostgreSQL · MySQL · SQLite", icon: "database" },
      { id: "records", x: 24, y: 352, w: 248, title: "Records", sub: "changes and loaded relations", icon: "braces" },
      { id: "events", x: 24, y: 440, w: 248, title: "Events and hooks", sub: "run as a record is saved", icon: "event" },
      { id: "after", x: 24, y: 528, w: 248, title: "After the commit", sub: "audit trail, broadcasts", icon: "broadcast" },
    ],
    edges: [
      { from: "model.bottom", to: "builder.top", step: 0 },
      { from: "builder.bottom", to: "uow.top", step: 1 },
      { from: "uow.bottom", to: "db.top", step: 2 },
      { from: "db.bottom", to: "records.top", step: 3 },
      { from: "records.bottom", to: "events.top", kind: "async" },
      { from: "events.bottom", to: "after.top", kind: "async" },
    ],
  },
};
