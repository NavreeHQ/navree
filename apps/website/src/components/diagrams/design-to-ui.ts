import type { DiagramSpec } from "./figure";

/**
 * Bringing a design into Navree UI: the phases of the navree-design-to-ui
 * skill in three stretches, each ending at a stop point where the owner
 * confirms. Wide: the stretches turn across two columns, each listing its
 * phases and the `design/` folder each one writes. Narrow: a column.
 */
export const designToUi: DiagramSpec = {
  title: "From a design to Navree UI pages",
  desc: "The phases of the navree-design-to-ui skill, in three stretches. Reading the design: 0 intake copies it to design/source and writes design/00-intake, 1 inventory writes design/01-inventory with the design's screenshots. Stop 1: the owner confirms the inventory and chooses keep or translate per screen. Mapping to Navree UI: 2 tokens writes design/02-tokens, 3 mapping writes design/03-mapping with a spec per page, 4 reconcile writes design/04-reconcile with the questions, deviations and gaps. Stop 2: the owner approves the page specs, deviations and gaps. Build and verify: 5 build writes the pages and design/05-build, 6 verify writes design/06-verify with screenshots beside the design. Stop 3: the owner looks at the pages beside the design and accepts them.",
  legend: [{ kind: "flow", label: "a stop: the owner confirms" }],
  wide: {
    width: 592,
    height: 344,
    nodes: [
      {
        id: "read",
        x: 0,
        y: 16,
        w: 264,
        h: 128,
        title: "Read the design",
        icon: "browser",
        rows: [
          ["0 Intake", "source/, 00-intake/"],
          ["1 Inventory", "01-inventory/"],
        ],
      },
      {
        id: "map",
        x: 328,
        y: 0,
        w: 264,
        h: 160,
        title: "Map to Navree UI",
        icon: "layers",
        rows: [
          ["2 Tokens", "02-tokens/"],
          ["3 Mapping", "03-mapping/"],
          ["4 Reconcile", "04-reconcile/"],
        ],
      },
      {
        id: "build",
        x: 328,
        y: 216,
        w: 264,
        h: 128,
        title: "Build and verify",
        icon: "code",
        rows: [
          ["5 Build", "05-build/"],
          ["6 Verify", "06-verify/"],
        ],
      },
      { id: "done", x: 0, y: 252, w: 264, title: "Accepted pages", sub: "seen beside the design", icon: "browser", tone: "accent" },
    ],
    edges: [
      { from: "read.right", to: "map.left", label: "stop 1", step: 0 },
      { from: "map.bottom", to: "build.top", label: "stop 2", step: 1 },
      { from: "build.left", to: "done.right", label: "stop 3", step: 2 },
    ],
  },
  narrow: {
    width: 272,
    height: 368,
    nodes: [
      { id: "read", x: 0, y: 0, w: 272, title: "Read the design", sub: "0 intake, 1 inventory", icon: "browser" },
      { id: "map", x: 0, y: 104, w: 272, title: "Map to Navree UI", sub: "2 tokens, 3 mapping, 4 reconcile", icon: "layers" },
      { id: "build", x: 0, y: 208, w: 272, title: "Build and verify", sub: "5 build, 6 verify", icon: "code" },
      { id: "done", x: 0, y: 312, w: 272, title: "Accepted pages", sub: "seen beside the design", icon: "browser", tone: "accent" },
    ],
    edges: [
      { from: "read.bottom", to: "map.top", label: "stop 1", step: 0 },
      { from: "map.bottom", to: "build.top", label: "stop 2", step: 1 },
      { from: "build.bottom", to: "done.top", label: "stop 3", step: 2 },
    ],
  },
};
