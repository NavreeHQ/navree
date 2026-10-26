import type { DiagramSpec } from "./figure";

/**
 * Backend for frontend: the browser holds no token. The BFF is the accent:
 * it keeps both tokens in httpOnly cookies and rotates the refresh token.
 */
export const backendForFrontend: DiagramSpec = {
  title: "Backend for frontend",
  desc: "The browser calls only its own web server, on the same origin, and holds cookies it can't read. The Next.js BFF keeps the access token (navree_at) and the refresh token (navree_rt) in httpOnly cookies and calls the Navree API with Authorization: Bearer. When the access token expires it refreshes once per refresh token, and Navree rotates the refresh token on every use. The Navree API reads and writes PostgreSQL in one transaction per request, and after a change it broadcasts to its WebSocket server, which pushes the event straight to the browser. The socket carries no token; private channels are authorized through the BFF.",
  legend: [
    { kind: "flow", label: "request" },
    { kind: "async", label: "real-time event" },
  ],
  wide: {
    width: 592,
    height: 504,
    nodes: [
      { id: "browser", x: 200, y: 0, w: 192, title: "Browser", sub: "cookies it can't read", icon: "browser" },
      {
        id: "bff",
        x: 200,
        y: 120,
        w: 192,
        h: 128,
        title: "Next.js BFF",
        sub: "httpOnly cookies",
        icon: "lock",
        tone: "accent",
        rows: [
          ["navree_at", "access"],
          ["navree_rt", "refresh"],
        ],
      },
      { id: "api", x: 200, y: 320, w: 192, title: "Navree API", sub: "bearer tokens only", icon: "server" },
      { id: "ws", x: 432, y: 320, w: 160, title: "Real time", sub: "WebSocket server", icon: "broadcast" },
      { id: "db", x: 200, y: 448, w: 192, title: "PostgreSQL", sub: "users · token families", icon: "database" },
    ],
    edges: [
      { from: "browser.bottom", to: "bff.top", label: "same origin", labelMono: false, at: [304, 92], step: 0 },
      { from: "bff.bottom", to: "api.top", label: "Authorization: Bearer", at: [304, 288], step: 1 },
      {
        from: "bff.left",
        to: "api.left",
        bend: 168,
        label: "refresh: rotates,\nonce per token",
        labelMono: false,
        at: [160, 260],
        anchor: "end",
      },
      { from: "api.bottom", to: "db.top", label: "one transaction", labelMono: false, at: [304, 420], step: 2 },
      { from: "api.right", to: "ws.left", kind: "async", step: 3 },
      {
        from: "ws.top",
        to: "browser.right",
        kind: "async",
        label: "no token\non the socket",
        labelMono: false,
        at: [504, 168],
        anchor: "end",
        step: 4,
      },
    ],
  },
  narrow: {
    width: 272,
    height: 504,
    nodes: [
      { id: "browser", x: 48, y: 0, w: 192, title: "Browser", sub: "cookies it can't read", icon: "browser" },
      {
        id: "bff",
        x: 48,
        y: 120,
        w: 192,
        h: 128,
        title: "Next.js BFF",
        sub: "httpOnly cookies",
        icon: "lock",
        tone: "accent",
        rows: [
          ["navree_at", "access"],
          ["navree_rt", "refresh"],
        ],
      },
      { id: "api", x: 48, y: 320, w: 192, title: "Navree API", sub: "HTTP and WebSocket", icon: "server" },
      { id: "db", x: 48, y: 448, w: 192, title: "PostgreSQL", sub: "users · token families", icon: "database" },
    ],
    edges: [
      { from: "browser.bottom", to: "bff.top", label: "same origin", labelMono: false, at: [152, 92], step: 0 },
      { from: "bff.bottom", to: "api.top", label: "Bearer", at: [152, 288], step: 1 },
      { from: "bff.left", to: "api.left", bend: 24, label: "refresh,\nrotates", labelMono: false, at: [32, 276] },
      { from: "api.bottom", to: "db.top", label: "one transaction", labelMono: false, at: [152, 420], step: 2 },
      { from: "api.right", to: "browser.right", kind: "async", bend: 256, step: 3 },
    ],
  },
};
