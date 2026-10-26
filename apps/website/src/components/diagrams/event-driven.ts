import type { DiagramSpec } from "./figure";

/**
 * Event-driven: the unit of work with its outbox is the accent. What
 * follows a change is written in the same transaction and leaves after
 * the commit.
 */
export const eventDriven: DiagramSpec = {
  title: "Event-driven, with an outbox",
  desc: "A request moves a deal to won. DealService emits DealWon. A sync listener runs at once, inside the same transaction. Everything else is written into the request's unit of work: the deal row, a jobs row for each queued listener and a micro_outbox row for other services, all committed in one transaction. After the commit, a worker runs the queued listener with retries, the deal.updated broadcast reaches browsers over the WebSocket server, and deal.won is published to other services through the broker. A rollback sends nothing.",
  legend: [
    { kind: "flow", label: "in the request" },
    { kind: "async", label: "after the commit" },
  ],
  wide: {
    width: 592,
    height: 280,
    nodes: [
      { id: "req", x: 0, y: 0, w: 168, title: "Request", sub: "move a deal to won", icon: "request" },
      { id: "svc", x: 0, y: 112, w: 168, title: "DealService", mono: true, sub: "emits DealWon", icon: "event" },
      { id: "sync", x: 0, y: 224, w: 168, title: "Sync listener", sub: "same transaction", icon: "worker" },
      {
        id: "uow",
        x: 208,
        y: 56,
        w: 184,
        h: 168,
        title: "Unit of work",
        sub: "one transaction",
        icon: "database",
        tone: "accent",
        rows: [
          ["deals", "change"],
          ["jobs", "listeners"],
          ["micro_outbox", "events"],
        ],
      },
      { id: "queued", x: 424, y: 40, w: 168, title: "Queued listener", sub: "queue:work · retries", icon: "queue" },
      { id: "broadcast", x: 424, y: 112, w: 168, title: "Broadcast", sub: "browsers, live", icon: "broadcast" },
      { id: "services", x: 424, y: 184, w: 168, title: "Other services", sub: "deal.won · broker", icon: "send" },
    ],
    edges: [
      { from: "req.bottom", to: "svc.top", step: 0 },
      { from: "svc.right", to: "uow.left", label: "emit", at: [188, 132], anchor: "middle", step: 1 },
      { from: "svc.bottom", to: "sync.top", step: 1 },
      { from: "uow.right", to: "queued.left", kind: "async", bend: 408, step: 2 },
      { from: "uow.right", to: "broadcast.left", kind: "async", bend: 408, step: 2 },
      { from: "uow.right", to: "services.left", kind: "async", bend: 408, step: 2 },
    ],
  },
  narrow: {
    width: 272,
    height: 600,
    nodes: [
      { id: "req", x: 0, y: 0, w: 120, title: "Request", sub: "deal → won" },
      { id: "svc", x: 0, y: 96, w: 120, title: "DealService", mono: true, sub: "emits DealWon" },
      { id: "sync", x: 152, y: 96, w: 120, title: "Sync listener", sub: "same transaction" },
      {
        id: "uow",
        x: 24,
        y: 192,
        w: 248,
        h: 168,
        title: "Unit of work",
        sub: "one transaction",
        icon: "database",
        tone: "accent",
        rows: [
          ["deals", "change"],
          ["jobs", "listeners"],
          ["micro_outbox", "events"],
        ],
      },
      { id: "queued", x: 24, y: 400, w: 248, title: "Queued listener", sub: "queue:work · retries", icon: "queue" },
      { id: "broadcast", x: 24, y: 472, w: 248, title: "Broadcast", sub: "browsers, live", icon: "broadcast" },
      { id: "services", x: 24, y: 544, w: 248, title: "Other services", sub: "deal.won · broker", icon: "send" },
    ],
    edges: [
      { from: "req.bottom", to: "svc.top", step: 0 },
      { from: "svc.right", to: "sync.left", step: 1 },
      { from: "svc.bottom", to: "uow.top", label: "emit", at: [156, 180], step: 1 },
      { from: "uow.left", to: "queued.left", kind: "async", bend: 8, step: 2 },
      { from: "uow.left", to: "broadcast.left", kind: "async", bend: 8, step: 2 },
      { from: "uow.left", to: "services.left", kind: "async", bend: 8, step: 2 },
    ],
  },
};
