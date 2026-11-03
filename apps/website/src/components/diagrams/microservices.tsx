import type { DiagramSpec } from "./figure";

/**
 * A trace waterfall: one request followed through every service, the way
 * an OpenTelemetry backend shows it. Bars are offsets in time.
 */
const spans = [
  { name: "crm-api", from: 0, to: 0.6 },
  { name: "deals", from: 0.1, to: 0.5 },
  { name: "notifications", from: 0.56, to: 0.96 },
  { name: "activities", from: 0.58, to: 0.82 },
] as const;

/** Stacked: each label sits above its bar (a narrow panel). */
function stackedTrace(x: number, y: number, w: number) {
  const track = w - 32;
  return (
    <g aria-hidden="true">
      <text data-label x={x + 16} y={y + 42} fontSize={12} className="fill-fg-2 font-sans">
        OpenTelemetry
      </text>
      {spans.map((s, i) => {
        const top = y + 72 + i * 38;
        return (
          <g key={s.name}>
            <text data-label x={x + 16} y={top} fontSize={12} className="fill-fg-2 font-mono">
              {s.name}
            </text>
            <rect x={x + 16} y={top + 8} width={track} height={6} rx={3} className="fill-ink-3" />
            <rect
              x={x + 16 + track * s.from}
              y={top + 8}
              width={track * (s.to - s.from)}
              height={6}
              rx={3}
              className="fill-fg-3/70"
            />
          </g>
        );
      })}
    </g>
  );
}

/** In rows: the label left, the bar right (a wide panel). */
function rowTrace(x: number, y: number, w: number) {
  const left = x + 16;
  const barX = x + 128;
  const track = w - 128 - 16;
  return (
    <g aria-hidden="true">
      {spans.map((s, i) => {
        const base = y + 56 + i * 24;
        return (
          <g key={s.name}>
            <text data-label x={left} y={base} fontSize={12} className="fill-fg-2 font-mono">
              {s.name}
            </text>
            <rect x={barX} y={base - 7} width={track} height={6} rx={3} className="fill-ink-3" />
            <rect
              x={barX + track * s.from}
              y={base - 7}
              width={track * (s.to - s.from)}
              height={6}
              rx={3}
              className="fill-fg-3/70"
            />
          </g>
        );
      })}
    </g>
  );
}

/**
 * Services split from modules: request-response messages and events over a
 * broker, a database per service and one trace across all of them. The
 * broker is the accent.
 */
export const microservices: DiagramSpec = {
  title: "Microservices",
  desc: "A web app calls crm-api, the HTTP gateway. crm-api asks the deals service for a deal with a request-response message (deals.get). When a deal is won, the deals service emits deal.won to a broker (NATS, Kafka, RabbitMQ or Redis Streams), which delivers it to the notifications and activities services. Each service owns its database. One OpenTelemetry trace follows the request through crm-api, deals, notifications and activities.",
  legend: [
    { kind: "flow", label: "request-response" },
    { kind: "async", label: "event" },
  ],
  wide: {
    width: 592,
    height: 496,
    groups: [{ id: "trace", x: 432, y: 0, w: 160, h: 216, label: "One trace" }],
    nodes: [
      { id: "web", x: 0, y: 0, w: 160, title: "Web app", sub: "browser", icon: "browser" },
      { id: "api", x: 216, y: 0, w: 160, title: "crm-api", mono: true, sub: "HTTP gateway", icon: "server" },
      {
        id: "deals",
        x: 216,
        y: 128,
        w: 160,
        h: 96,
        title: "deals",
        mono: true,
        sub: "answers deals.*",
        icon: "server",
        rows: ["own database"],
      },
      {
        id: "broker",
        x: 176,
        y: 288,
        w: 240,
        title: "Broker",
        sub: "NATS · Kafka · RabbitMQ · Redis",
        icon: "broker",
        tone: "accent",
      },
      {
        id: "notifications",
        x: 0,
        y: 400,
        w: 176,
        h: 96,
        title: "notifications",
        mono: true,
        sub: "handles deal.won",
        icon: "server",
        rows: ["own database"],
      },
      {
        id: "activities",
        x: 416,
        y: 400,
        w: 176,
        h: 96,
        title: "activities",
        mono: true,
        sub: "handles deal.won",
        icon: "server",
        rows: ["own database"],
      },
    ],
    edges: [
      { from: "web.right", to: "api.left", label: "HTTPS", at: [188, 20], anchor: "middle", step: 0 },
      { from: "api.bottom", to: "deals.top", label: "send · deals.get", at: [304, 96], step: 1 },
      { from: "deals.bottom", to: "broker.top", kind: "async", label: "emit deal.won", at: [304, 260], step: 2 },
      { from: "broker.left", to: "notifications.top", kind: "async", label: "deal.won", at: [96, 364], step: 3 },
      { from: "broker.right", to: "activities.top", kind: "async", label: "deal.won", at: [512, 364], step: 3 },
    ],
    extras: stackedTrace(432, 0, 160),
  },
  narrow: {
    width: 272,
    height: 704,
    groups: [{ id: "trace", x: 0, y: 560, w: 272, h: 144, label: "One trace · OpenTelemetry" }],
    nodes: [
      { id: "web", x: 56, y: 0, w: 160, title: "Web app", sub: "browser", icon: "browser" },
      { id: "api", x: 56, y: 112, w: 160, title: "crm-api", mono: true, sub: "HTTP gateway", icon: "server" },
      { id: "deals", x: 56, y: 224, w: 160, title: "deals", mono: true, sub: "own database", icon: "server" },
      {
        id: "broker",
        x: 16,
        y: 344,
        w: 240,
        title: "Broker",
        sub: "NATS · Kafka · RabbitMQ · Redis",
        icon: "broker",
        tone: "accent",
      },
      { id: "notifications", x: 0, y: 464, w: 128, title: "notifications", mono: true, sub: "own database" },
      { id: "activities", x: 144, y: 464, w: 128, title: "activities", mono: true, sub: "own database" },
    ],
    edges: [
      { from: "web.bottom", to: "api.top", label: "HTTPS", at: [144, 88], step: 0 },
      { from: "api.bottom", to: "deals.top", label: "send · deals.get", at: [144, 200], step: 1 },
      { from: "deals.bottom", to: "broker.top", kind: "async", label: "emit deal.won", at: [144, 316], step: 2 },
      { from: "broker.bottom", to: "notifications.top", kind: "async", bend: 432, label: "deal.won", at: [144, 420], step: 3 },
      { from: "broker.bottom", to: "activities.top", kind: "async", bend: 432, step: 3 },
    ],
    extras: rowTrace(0, 560, 272),
  },
};
