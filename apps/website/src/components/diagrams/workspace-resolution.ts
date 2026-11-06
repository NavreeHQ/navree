import type { DiagramSpec } from "./figure";

/**
 * How a request finds its workspace. Wide: the main column is the request's
 * path, the resolvers in the order configured with the first match winning,
 * then the lookup, the membership check and `CurrentWorkspace` (the
 * accent); the dashed nodes beside it are the three answers that stop the
 * request. Narrow: the same column, with each refusal in the step's line.
 */
export const workspaceResolution: DiagramSpec = {
  title: "How a request finds its workspace",
  desc: "The resolvers in workspace.resolvers run in order and the first one that finds a name wins: the header X-Workspace, the path after /w, the first label of the host under base_domain, or the workspace claim of the bearer token. The domain and session resolvers join the list when you add them. A request that names no workspace, and has no credential pinned to one, is refused with 400 workspace_required. The name is looked up by id when it is a UUID and by slug otherwise, and an unknown workspace is a 404 workspace_not_found. Navree then checks that the signed-in user is a member, holds an organization grant or is a guest of a demo workspace, and refuses anyone else with 403 not_a_member. The handler receives the workspace and the membership as CurrentWorkspace.",
  legend: [{ kind: "flow", label: "request" }],
  wide: {
    width: 592,
    height: 616,
    nodes: [
      { id: "req", x: 0, y: 0, w: 352, title: "Request", sub: "names its workspace somewhere", icon: "request" },
      {
        id: "resolvers",
        x: 0,
        y: 104,
        w: 352,
        h: 200,
        title: "Resolvers, in order",
        sub: "the first that finds a name wins",
        icon: "funnel",
        rows: [
          ["header", "X-Workspace: sutlej"],
          ["path", "/w/sutlej/deals"],
          ["subdomain", "sutlej.app.example.com"],
          ["claim", "workspace: sutlej"],
        ],
      },
      { id: "lookup", x: 0, y: 352, w: 352, title: "Look up the workspace", sub: "a UUID by id, anything else by slug", icon: "database" },
      { id: "admit", x: 0, y: 456, w: 352, title: "Check admission", sub: "member, organization grant or demo guest", icon: "users" },
      { id: "current", x: 0, y: 560, w: 352, title: "CurrentWorkspace", mono: true, sub: "the workspace and the membership", icon: "code", tone: "accent" },
      { id: "e400", x: 416, y: 176, w: 176, title: "400", mono: true, sub: "workspace_required", subMono: true, tone: "muted" },
      { id: "e404", x: 416, y: 352, w: 176, title: "404", mono: true, sub: "workspace_not_found", subMono: true, tone: "muted" },
      { id: "e403", x: 416, y: 456, w: 176, title: "403", mono: true, sub: "not_a_member", subMono: true, tone: "muted" },
    ],
    edges: [
      { from: "req.bottom", to: "resolvers.top", step: 0 },
      { from: "resolvers.bottom", to: "lookup.top", step: 1 },
      { from: "lookup.bottom", to: "admit.top", step: 2 },
      { from: "admit.bottom", to: "current.top", step: 3 },
      { from: "resolvers.right", to: "e400.left", label: "no name", labelMono: false, at: [368, 196] },
      { from: "lookup.right", to: "e404.left" },
      { from: "admit.right", to: "e403.left" },
    ],
    notes: [{ x: 192, y: 332, text: "domain and session join the list when you add them", tone: "fg-3" }],
  },
  narrow: {
    width: 272,
    height: 624,
    nodes: [
      { id: "req", x: 24, y: 0, w: 248, title: "Request", sub: "names its workspace", icon: "request" },
      {
        id: "resolvers",
        x: 24,
        y: 96,
        w: 248,
        h: 200,
        title: "Resolvers, in order",
        sub: "first match wins · else 400",
        icon: "funnel",
        rows: [
          ["header", "X-Workspace"],
          ["path", "/w/sutlej"],
          ["subdomain", "sutlej.app…"],
          ["claim", "workspace"],
        ],
      },
      { id: "lookup", x: 24, y: 344, w: 248, title: "Look up", sub: "UUID by id, else slug · 404", icon: "database" },
      { id: "admit", x: 24, y: 440, w: 248, title: "Check admission", sub: "member, grant or guest · 403", icon: "users" },
      { id: "current", x: 24, y: 536, w: 248, title: "CurrentWorkspace", mono: true, sub: "the handler's extractor", icon: "code", tone: "accent" },
    ],
    edges: [
      { from: "req.bottom", to: "resolvers.top", step: 0 },
      { from: "resolvers.bottom", to: "lookup.top", step: 1 },
      { from: "lookup.bottom", to: "admit.top", step: 2 },
      { from: "admit.bottom", to: "current.top", step: 3 },
    ],
    notes: [
      { x: 156, y: 316, text: "domain and session", tone: "fg-3" },
      { x: 156, y: 332, text: "can join the list", tone: "fg-3" },
    ],
  },
};
