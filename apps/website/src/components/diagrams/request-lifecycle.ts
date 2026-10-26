import type { DiagramSpec } from "./figure";

/**
 * The request pipeline. Wide: the layers nest around the handler (the
 * accent), and the request goes in through each one and the response back
 * out through them in reverse. Narrow: the same order as a list.
 */
export const requestLifecycle: DiagramSpec = {
  title: "The life of a request",
  desc: "A request passes through the global middleware (request id, CORS, timeouts, security headers, rate and body limits), then the unit of work, which opens a transaction. The route's access rules run next: authentication, throttling, guards and authorization, ending the request with 401, 403 or 429. Extractors build the handler's arguments and validate the body (Valid<Json<T>>, 400 on failure). The handler runs. On the way back out the unit of work commits when the status is below 400 and rolls back otherwise, and the response passes back through the middleware.",
  legend: [{ kind: "flow", label: "request and response" }],
  wide: {
    width: 592,
    height: 280,
    groups: [
      { id: "middleware", x: 112, y: 0, w: 368, h: 280, label: "Middleware" },
      { id: "uow", x: 128, y: 40, w: 336, h: 224, label: "Unit of work" },
      { id: "access", x: 144, y: 80, w: 304, h: 168, label: "Access rules" },
      { id: "extract", x: 160, y: 120, w: 272, h: 112, label: "Extractors" },
    ],
    nodes: [
      { id: "req", x: 0, y: 168, w: 88, h: 40, title: "Request" },
      {
        id: "handler",
        x: 176,
        y: 160,
        w: 240,
        title: "Handler",
        sub: "DealsController::update",
        subMono: true,
        icon: "code",
        tone: "accent",
      },
      { id: "resp", x: 504, y: 168, w: 88, h: 40, title: "Response" },
    ],
    edges: [
      { from: "req.right", to: "handler.left", step: 0 },
      { from: "handler.right", to: "resp.left", step: 1 },
    ],
    notes: [
      { x: 464, y: 22, text: "request id · CORS · timeouts · limits", anchor: "end" },
      { x: 448, y: 62, text: "commits below 400", anchor: "end" },
      { x: 432, y: 102, text: "401 · 403 · 429", anchor: "end", mono: true },
      { x: 416, y: 142, text: "Valid<Json<T>>", anchor: "end", mono: true },
    ],
  },
  narrow: {
    width: 272,
    height: 584,
    nodes: [
      { id: "req", x: 56, y: 0, w: 160, h: 40, title: "Request" },
      { id: "mw", x: 16, y: 64, w: 240, title: "Middleware", sub: "request id · CORS · limits", icon: "layers" },
      { id: "open", x: 16, y: 144, w: 240, title: "Unit of work", sub: "opens a transaction", icon: "database" },
      { id: "access", x: 16, y: 224, w: 240, title: "Access rules", sub: "401 · 403 · 429", subMono: true, icon: "shield" },
      { id: "extract", x: 16, y: 304, w: 240, title: "Extractors", sub: "Valid<Json<T>>", subMono: true, icon: "braces" },
      {
        id: "handler",
        x: 16,
        y: 384,
        w: 240,
        title: "Handler",
        sub: "DealsController::update",
        subMono: true,
        icon: "code",
        tone: "accent",
      },
      { id: "commit", x: 16, y: 464, w: 240, title: "Unit of work", sub: "commits below 400", icon: "database" },
      { id: "resp", x: 56, y: 544, w: 160, h: 40, title: "Response" },
    ],
    edges: [
      { from: "req.bottom", to: "mw.top", step: 0 },
      { from: "mw.bottom", to: "open.top", step: 1 },
      { from: "open.bottom", to: "access.top", step: 2 },
      { from: "access.bottom", to: "extract.top", step: 3 },
      { from: "extract.bottom", to: "handler.top", step: 4 },
      { from: "handler.bottom", to: "commit.top", step: 5 },
      { from: "commit.bottom", to: "resp.top", step: 6 },
    ],
  },
};
