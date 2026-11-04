import type { DiagramSpec } from "./figure";

/**
 * From a request to pixels, with Navree UI. Wide: on the server a handler
 * builds one typed tree and `render` writes it to HTML with the page's
 * content security policy; in the browser the page loads only the modules
 * its markup uses. A live region (the accent) follows an event stream: an
 * event sends it back to a handler for its HTML, which morphs in place.
 * Narrow: the same steps as a column, the event stream last.
 */
export const requestToPixels: DiagramSpec = {
  title: "From a request to pixels",
  desc: "On the server, a Rust handler returns components, which build one typed Node tree. render writes the tree to HTML in one pass, sent with a content security policy that allows no inline script or style. In the browser, the page loads only the script modules its markup uses, and none when nothing on it is interactive. A live region follows a stream of server-sent events. An event carries nothing the page reads: it tells the region to fetch its HTML again from a handler, and the new HTML morphs into the region in place, keeping focus, the caret, the selection and what was typed.",
  legend: [
    { kind: "flow", label: "the page" },
    { kind: "async", label: "a live update" },
  ],
  wide: {
    width: 592,
    height: 416,
    groups: [
      { id: "server", x: 0, y: 0, w: 592, h: 184, label: "On the server" },
      { id: "browser", x: 0, y: 304, w: 592, h: 112, label: "In the browser" },
    ],
    nodes: [
      { id: "handler", x: 16, y: 40, w: 176, title: "Handler", sub: "returns components", icon: "code" },
      { id: "tree", x: 208, y: 40, w: 176, title: "Node", mono: true, sub: "one typed tree", icon: "braces" },
      { id: "render", x: 400, y: 40, w: 176, title: "render", mono: true, sub: "HTML in one pass", icon: "document" },
      { id: "events", x: 16, y: 112, w: 176, title: "Event stream", sub: "server-sent events", icon: "broadcast" },
      { id: "live", x: 16, y: 344, w: 176, title: "Live region", sub: "morphs in place", icon: "event", tone: "accent" },
      { id: "modules", x: 208, y: 344, w: 176, title: "Modules", sub: "only what it uses", icon: "layers" },
      { id: "page", x: 400, y: 344, w: 176, title: "Page", sub: "no inline code", icon: "browser" },
    ],
    edges: [
      { from: "handler.right", to: "tree.left", step: 0 },
      { from: "tree.right", to: "render.left", step: 1 },
      { from: "render.bottom", to: "page.top", step: 2, label: "HTML + CSP", at: [496, 248] },
      { from: "page.left", to: "modules.right", step: 3 },
      { from: "modules.left", to: "live.right", step: 4 },
      // Down the left margin, clear of the browser group's label.
      {
        from: "events.left",
        to: "live.left",
        kind: "async",
        step: 5,
        via: [
          [6, 140],
          [6, 372],
        ],
        label: "an event → a refetch",
        at: [16, 248],
      },
    ],
  },
  narrow: {
    width: 272,
    height: 608,
    nodes: [
      { id: "handler", x: 16, y: 0, w: 240, title: "Handler", sub: "returns components", icon: "code" },
      { id: "tree", x: 16, y: 88, w: 240, title: "Node", mono: true, sub: "one typed tree", icon: "braces" },
      { id: "render", x: 16, y: 176, w: 240, title: "render", mono: true, sub: "HTML in one pass", icon: "document" },
      { id: "page", x: 16, y: 264, w: 240, title: "Page", sub: "no inline code", icon: "browser" },
      { id: "modules", x: 16, y: 352, w: 240, title: "Modules", sub: "only what the markup uses", icon: "layers" },
      { id: "live", x: 16, y: 440, w: 240, title: "Live region", sub: "morphs in place", icon: "event", tone: "accent" },
      { id: "events", x: 16, y: 552, w: 240, title: "Event stream", sub: "server-sent events", icon: "broadcast" },
    ],
    edges: [
      { from: "handler.bottom", to: "tree.top", step: 0 },
      { from: "tree.bottom", to: "render.top", step: 1 },
      { from: "render.bottom", to: "page.top", step: 2, label: "HTML + CSP", at: [152, 252] },
      { from: "page.bottom", to: "modules.top", step: 3 },
      { from: "modules.bottom", to: "live.top", step: 4 },
      { from: "events.top", to: "live.bottom", kind: "async", step: 5, label: "an event\n→ a refetch", at: [152, 519] },
    ],
  },
};
