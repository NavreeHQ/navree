import type { DiagramSpec } from "./figure";

/**
 * How the two skills work together. Wide: the old product feeds
 * navree-reverse-engineering (its code and data) and the look (its
 * screenshots); the reverse-engineering specs for screens, records and
 * lists feed navree-design-to-ui; both end in the one Navree app (the
 * accent). Narrow: the same chain as a column.
 */
export const twoWays: DiagramSpec = {
  title: "The two skills together",
  desc: "The old product's code and data go to the navree-reverse-engineering skill, which writes specs and builds the models, services and routes of the Navree app. The look comes from the old product's screenshots (SHOT- captures, which become the design's BASE- screenshots) or from a new design, and goes to the navree-design-to-ui skill. The reverse-engineering specs screens.md, crud.md and lists.md feed the design skill's page specs, so it adds only the look. The design skill builds the app's Navree UI pages, module by module, after each module's routes and services pass their gate.",
  legend: [{ kind: "flow", label: "what each step hands on" }],
  wide: {
    width: 592,
    height: 328,
    nodes: [
      { id: "old", x: 0, y: 0, w: 240, title: "Old product", sub: "code, data, screens", icon: "server" },
      { id: "look", x: 352, y: 0, w: 240, title: "The look", sub: "old screens or a new design", icon: "browser" },
      { id: "re", x: 0, y: 136, w: 240, title: "navree-reverse-engineering", mono: true, sub: "specs/" , subMono: true },
      { id: "dtu", x: 352, y: 136, w: 240, title: "navree-design-to-ui", mono: true, sub: "design/", subMono: true },
      { id: "app", x: 176, y: 272, w: 240, title: "The Navree app", sub: "one module at a time", icon: "module", tone: "accent" },
    ],
    edges: [
      { from: "old.bottom", to: "re.top", label: "code and data", labelMono: false },
      { from: "old.right", to: "look.left", label: "SHOT-*" },
      { from: "look.bottom", to: "dtu.top", label: "BASE-*" },
      {
        from: "re.right",
        to: "dtu.left",
        label: "screens.md\ncrud.md\nlists.md",
        at: [296, 116],
        anchor: "middle",
      },
      { from: "re.bottom", to: "app.left", label: "models, services, routes", labelMono: false, at: [128, 240] },
      { from: "dtu.bottom", to: "app.right", label: "the pages", labelMono: false, at: [480, 240] },
    ],
  },
  narrow: {
    width: 272,
    height: 368,
    nodes: [
      { id: "old", x: 0, y: 0, w: 272, title: "Old product", sub: "code, data, screens", icon: "server" },
      { id: "re", x: 0, y: 104, w: 272, title: "navree-reverse-engineering", mono: true, sub: "specs/: screens, crud, lists" },
      { id: "dtu", x: 0, y: 208, w: 272, title: "navree-design-to-ui", mono: true, sub: "the look: old screens or a design" },
      { id: "app", x: 0, y: 312, w: 272, title: "The Navree app", sub: "one module at a time", icon: "module", tone: "accent" },
    ],
    edges: [
      { from: "old.bottom", to: "re.top", label: "code and data", labelMono: false },
      { from: "re.bottom", to: "dtu.top", label: "the screen specs", labelMono: false },
      { from: "dtu.bottom", to: "app.top", label: "the pages", labelMono: false },
    ],
  },
};
