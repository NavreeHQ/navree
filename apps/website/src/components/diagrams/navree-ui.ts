import type { DiagramSpec } from "./figure";

/**
 * How Navree UI works. Wide: on the server a handler builds one typed tree,
 * which renders to HTML in one pass (the accent); in the browser the page
 * loads the stylesheet, and modules only when its markup uses them. An
 * action posts back and gets ops. Narrow: the same steps as a column.
 */
export const navreeUi: DiagramSpec = {
  title: "How Navree UI works",
  desc: "On the server, a Rust handler returns components, which build one typed Node tree. render writes the tree to HTML in one pass, with the page's content security policy. In the browser, every page loads navree-ui.css; a module script (core, overlay, sidebar or table) loads only when the page's markup uses it. An action, such as a form post, goes back to a handler, which answers with a Reply: ops that morph the page, or a redirect or a full page when JavaScript is off.",
  legend: [
    { kind: "flow", label: "a page" },
    { kind: "async", label: "an action" },
  ],
  wide: {
    width: 592,
    height: 344,
    groups: [
      { id: "server", x: 0, y: 0, w: 592, h: 112, label: "On the server" },
      { id: "browser", x: 0, y: 232, w: 592, h: 112, label: "In the browser" },
    ],
    nodes: [
      { id: "handler", x: 16, y: 40, w: 176, title: "Handler", sub: "returns components", icon: "code" },
      { id: "tree", x: 208, y: 40, w: 176, title: "Node", sub: "one typed tree", icon: "braces" },
      { id: "render", x: 400, y: 40, w: 176, title: "render", mono: true, sub: "HTML in one pass", icon: "document", tone: "accent" },
      { id: "mods", x: 16, y: 272, w: 176, title: "Modules", sub: "only when used", icon: "layers" },
      { id: "css", x: 208, y: 272, w: 176, title: "navree-ui.css", mono: true, sub: "every page", icon: "browser" },
      { id: "page", x: 400, y: 272, w: 176, title: "Page", sub: "data-slot markup", subMono: false, icon: "browser" },
    ],
    edges: [
      { from: "handler.right", to: "tree.left", step: 0 },
      { from: "tree.right", to: "render.left", step: 1 },
      { from: "render.bottom", to: "page.top", step: 2, label: "HTML + CSP", at: [496, 176] },
      { from: "page.left", to: "css.right" },
      { from: "css.left", to: "mods.right" },
      // Down the left margin, clear of the groups' labels.
      {
        from: "mods.left",
        to: "handler.left",
        kind: "async",
        via: [
          [6, 304],
          [6, 72],
        ],
        label: "an action → Reply, ops",
        at: [16, 176],
      },
    ],
  },
  narrow: {
    width: 272,
    height: 504,
    nodes: [
      { id: "handler", x: 16, y: 0, w: 240, title: "Handler", sub: "returns components", icon: "code" },
      { id: "tree", x: 16, y: 88, w: 240, title: "Node", sub: "one typed tree", icon: "braces" },
      { id: "render", x: 16, y: 176, w: 240, title: "render", mono: true, sub: "HTML + CSP in one pass", icon: "document", tone: "accent" },
      { id: "page", x: 16, y: 264, w: 240, title: "Page", sub: "loads navree-ui.css", icon: "browser" },
      { id: "mods", x: 16, y: 352, w: 240, title: "Modules", sub: "only when the markup uses them", icon: "layers" },
      { id: "reply", x: 16, y: 448, w: 240, title: "Reply", mono: true, sub: "ops that morph the page", icon: "send" },
    ],
    edges: [
      { from: "handler.bottom", to: "tree.top", step: 0 },
      { from: "tree.bottom", to: "render.top", step: 1 },
      { from: "render.bottom", to: "page.top", step: 2 },
      { from: "page.bottom", to: "mods.top" },
      { from: "mods.bottom", to: "reply.top", kind: "async", label: "an action", at: [144, 428] },
    ],
  },
};
