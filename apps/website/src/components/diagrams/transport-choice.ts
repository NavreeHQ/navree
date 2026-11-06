import type { DiagramSpec } from "./figure";

/**
 * One API, six transports. Wide: your code (the handlers and the client)
 * goes through MicroModule (the accent), whose `micro.transport` setting
 * picks one of the six, each labelled with what it is for. Narrow: the
 * same, as a column.
 */
export const transportChoice: DiagramSpec = {
  title: "One API, six transports",
  desc: "Your message and event handlers, and the ServiceClient that calls them, never name a transport. They go through MicroModule, which wraps every message in an envelope of a pattern, headers and a payload. The micro.transport setting picks how the envelope travels: TCP for two services without a broker, NATS for most systems with JetStream for durable events, Kafka for streams you can replay, RabbitMQ for routing or when you already run it, Redis Streams when you already run Redis, and MQTT for devices and the edge.",
  legend: [{ kind: "flow", label: "a message or an event" }],
  wide: {
    width: 592,
    height: 376,
    nodes: [
      { id: "handlers", x: 0, y: 96, w: 176, title: "Handlers", sub: "messages and events", icon: "code" },
      { id: "client", x: 0, y: 232, w: 176, title: "ServiceClient", mono: true, sub: "send and emit", icon: "send" },
      {
        id: "module",
        x: 208,
        y: 104,
        w: 176,
        h: 168,
        title: "MicroModule",
        mono: true,
        sub: "micro.transport",
        subMono: true,
        icon: "module",
        tone: "accent",
        rows: ["pattern", "headers", "payload"],
      },
      { id: "tcp", x: 416, y: 0, w: 176, title: "TCP", sub: "two services, no broker", align: "center" },
      { id: "nats", x: 416, y: 64, w: 176, title: "NATS", sub: "most systems; JetStream", align: "center" },
      { id: "kafka", x: 416, y: 128, w: 176, title: "Kafka", sub: "streams you can replay", align: "center" },
      { id: "rabbitmq", x: 416, y: 192, w: 176, title: "RabbitMQ", sub: "routing, or you run it", align: "center" },
      { id: "redis", x: 416, y: 256, w: 176, title: "Redis Streams", sub: "you already run Redis", align: "center" },
      { id: "mqtt", x: 416, y: 320, w: 176, title: "MQTT", sub: "devices and the edge", align: "center" },
    ],
    edges: [
      { from: "handlers.right", to: "module.left", bend: 192, step: 0 },
      { from: "client.right", to: "module.left", bend: 192, step: 0 },
      { from: "module.right", to: "tcp.left", bend: 400, step: 1 },
      { from: "module.right", to: "nats.left", bend: 400, step: 1 },
      { from: "module.right", to: "kafka.left", bend: 400, step: 1 },
      { from: "module.right", to: "rabbitmq.left", bend: 400, step: 1 },
      { from: "module.right", to: "redis.left", bend: 400, step: 1 },
      { from: "module.right", to: "mqtt.left", bend: 400, step: 1 },
    ],
  },
  narrow: {
    width: 272,
    height: 576,
    nodes: [
      { id: "code", x: 24, y: 0, w: 248, title: "Handlers and ServiceClient", sub: "they never name a transport", icon: "code" },
      { id: "module", x: 24, y: 88, w: 248, title: "MicroModule", mono: true, sub: "micro.transport", subMono: true, icon: "module", tone: "accent" },
      { id: "tcp", x: 24, y: 176, w: 248, title: "TCP", sub: "two services, no broker", align: "center" },
      { id: "nats", x: 24, y: 240, w: 248, title: "NATS", sub: "most systems; JetStream", align: "center" },
      { id: "kafka", x: 24, y: 304, w: 248, title: "Kafka", sub: "streams you can replay", align: "center" },
      { id: "rabbitmq", x: 24, y: 368, w: 248, title: "RabbitMQ", sub: "routing, or you run it", align: "center" },
      { id: "redis", x: 24, y: 432, w: 248, title: "Redis Streams", sub: "you already run Redis", align: "center" },
      { id: "mqtt", x: 24, y: 496, w: 248, title: "MQTT", sub: "devices and the edge", align: "center" },
    ],
    edges: [
      { from: "code.bottom", to: "module.top", step: 0 },
      { from: "module.left", to: "tcp.left", bend: 8, step: 1 },
      { from: "module.left", to: "nats.left", bend: 8, step: 1 },
      { from: "module.left", to: "kafka.left", bend: 8, step: 1 },
      { from: "module.left", to: "rabbitmq.left", bend: 8, step: 1 },
      { from: "module.left", to: "redis.left", bend: 8, step: 1 },
      { from: "module.left", to: "mqtt.left", bend: 8, step: 1 },
    ],
  },
};
