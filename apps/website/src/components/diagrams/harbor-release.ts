import type { DiagramSpec } from "./figure";

/**
 * What `navree deploy` does. Wide: three rows read as a snake, left to right
 * on the first, right to left on the second and left to right on the third;
 * the switch (the accent) is the moment traffic moves. A release that fails
 * its health check never gets there. Narrow: the same steps as a column.
 */
export const harborRelease: DiagramSpec = {
  title: "A Harbor release, step by step",
  desc: "navree deploy builds the release binary, uploads it to every server over SSH and runs the migrations once, from the new release. It starts the new release in the idle slot, green, beside the live one, blue, and requests /health/ready until it answers 200. If the check fails, the new instance is stopped and the old release keeps serving. Once it passes, Caddy is pointed at green and reloads gracefully, which is the switch. The old slot is then stopped after its requests in flight finish, the worker and scheduler processes restart on the new release, and releases beyond the last five are removed.",
  legend: [{ kind: "flow", label: "navree deploy" }],
  wide: {
    width: 592,
    height: 264,
    nodes: [
      { id: "build", x: 0, y: 0, w: 176, title: "Build", sub: "release binary", icon: "container" },
      { id: "upload", x: 208, y: 0, w: 176, title: "Upload", sub: "files, over SSH", icon: "send" },
      { id: "migrate", x: 416, y: 0, w: 176, title: "Migrate", sub: "once, new release", icon: "database" },
      { id: "start", x: 416, y: 104, w: 176, title: "Start", sub: "green, beside blue", icon: "server" },
      { id: "health", x: 208, y: 104, w: 176, title: "Health check", sub: "GET /health/ready", subMono: true, icon: "shield" },
      { id: "switch", x: 0, y: 104, w: 176, title: "Switch", sub: "Caddy reloads", icon: "route", tone: "accent" },
      { id: "drain", x: 0, y: 208, w: 176, title: "Drain", sub: "blue finishes first", icon: "queue" },
      { id: "restart", x: 208, y: 208, w: 176, title: "Restart workers", sub: "jobs finish first", icon: "worker" },
      { id: "prune", x: 416, y: 208, w: 176, title: "Prune", sub: "keeps the last 5", icon: "layers" },
    ],
    edges: [
      { from: "build.right", to: "upload.left", step: 0 },
      { from: "upload.right", to: "migrate.left", step: 1 },
      { from: "migrate.bottom", to: "start.top", step: 2 },
      { from: "start.left", to: "health.right", step: 3 },
      { from: "health.left", to: "switch.right", step: 4 },
      { from: "switch.bottom", to: "drain.top", step: 5 },
      { from: "drain.right", to: "restart.left", step: 6 },
      { from: "restart.right", to: "prune.left", step: 7 },
    ],
    notes: [{ x: 296, y: 192, text: "if it fails, blue keeps serving", anchor: "middle" }],
  },
  narrow: {
    width: 272,
    height: 584,
    nodes: [
      { id: "build", x: 24, y: 0, w: 248, title: "Build", sub: "the release binary", icon: "container" },
      { id: "upload", x: 24, y: 88, w: 248, title: "Upload", sub: "files, over SSH", icon: "send" },
      { id: "migrate", x: 24, y: 176, w: 248, title: "Migrate", sub: "once, from the new release", icon: "database" },
      { id: "start", x: 24, y: 264, w: 248, title: "Start", sub: "green, beside blue", icon: "server" },
      { id: "health", x: 24, y: 352, w: 248, title: "Health check", sub: "if it fails, blue keeps serving", icon: "shield" },
      { id: "switch", x: 24, y: 440, w: 248, title: "Switch", sub: "Caddy reloads", icon: "route", tone: "accent" },
      { id: "drain", x: 24, y: 528, w: 248, title: "Drain and restart", sub: "blue finishes, workers restart", icon: "queue" },
    ],
    edges: [
      { from: "build.bottom", to: "upload.top", step: 0 },
      { from: "upload.bottom", to: "migrate.top", step: 1 },
      { from: "migrate.bottom", to: "start.top", step: 2 },
      { from: "start.bottom", to: "health.top", step: 3 },
      { from: "health.bottom", to: "switch.top", step: 4 },
      { from: "switch.bottom", to: "drain.top", step: 5 },
    ],
  },
};
