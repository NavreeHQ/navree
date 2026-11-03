import type { DiagramSpec } from "./figure";

/**
 * Bringing an old product into Navree: the phases of the
 * navree-reverse-engineering skill in five stretches, each ending at a stop
 * point where the owner confirms. Wide: the stretches snake across two
 * columns, each listing its phases and the `specs/` folder each one writes;
 * every line between two stretches is a stop. Narrow: the same stretches in
 * a column, the phases in the sub-line.
 */
export const reverseEngineering: DiagramSpec = {
  title: "From an old product to a Navree app",
  desc: "The phases of the navree-reverse-engineering skill, in five stretches. Reading the product: 0 intake writes specs/00-intake, 1 map writes specs/01-map. Stop 1: the owner confirms the map. Schema and modules: 2 base layer writes specs/02-base, 3 modules writes specs/03-modules. Stop 2: the owner confirms the modules and their order. Specs: 4 module specs write specs/04-specs, 5 cross-cutting writes specs/05-cross, 6 data migration writes specs/06-migration, 7 reconcile writes specs/07-reconcile. Stop 3: the owner answers the questions. Plan: 8 tasks and tests writes specs/08-plan. Stop 4: the owner approves the plan. Build and verify: 9 build writes the Navree app and specs/09-build, 10 verify writes specs/10-verify. Stop 5: the owner accepts, then cut-over.",
  legend: [{ kind: "flow", label: "a stop: the owner confirms" }],
  wide: {
    width: 592,
    height: 512,
    nodes: [
      {
        id: "read",
        x: 0,
        y: 0,
        w: 264,
        h: 128,
        title: "Read the product",
        icon: "browser",
        rows: [
          ["0 Intake", "00-intake/"],
          ["1 Map", "01-map/"],
        ],
      },
      {
        id: "base",
        x: 328,
        y: 0,
        w: 264,
        h: 128,
        title: "Schema and modules",
        icon: "database",
        rows: [
          ["2 Base layer", "02-base/"],
          ["3 Modules", "03-modules/"],
        ],
      },
      {
        id: "specs",
        x: 328,
        y: 184,
        w: 264,
        h: 192,
        title: "Specs",
        icon: "document",
        rows: [
          ["4 Module specs", "04-specs/"],
          ["5 Cross-cutting", "05-cross/"],
          ["6 Data migration", "06-migration/"],
          ["7 Reconcile", "07-reconcile/"],
        ],
      },
      {
        id: "plan",
        x: 0,
        y: 232,
        w: 264,
        h: 96,
        title: "Plan",
        icon: "queue",
        rows: [["8 Tasks and tests", "08-plan/"]],
      },
      {
        id: "build",
        x: 0,
        y: 384,
        w: 264,
        h: 128,
        title: "Build and verify",
        icon: "code",
        rows: [
          ["9 Build", "09-build/"],
          ["10 Verify", "10-verify/"],
        ],
      },
      { id: "live", x: 328, y: 420, w: 264, title: "Cut-over", sub: "the Navree app opens", icon: "send", tone: "accent" },
    ],
    edges: [
      { from: "read.right", to: "base.left", label: "stop 1", step: 0 },
      { from: "base.bottom", to: "specs.top", label: "stop 2", step: 1 },
      { from: "specs.left", to: "plan.right", label: "stop 3", step: 2 },
      { from: "plan.bottom", to: "build.top", label: "stop 4", step: 3 },
      { from: "build.right", to: "live.left", label: "stop 5", step: 4 },
    ],
  },
  narrow: {
    width: 272,
    height: 576,
    nodes: [
      { id: "read", x: 0, y: 0, w: 272, title: "Read the product", sub: "0 intake, 1 map", icon: "browser" },
      { id: "base", x: 0, y: 104, w: 272, title: "Schema and modules", sub: "2 base layer, 3 modules", icon: "database" },
      { id: "specs", x: 0, y: 208, w: 272, title: "Specs", sub: "4 to 7: specs, data, reconcile", icon: "document" },
      { id: "plan", x: 0, y: 312, w: 272, title: "Plan", sub: "8 tasks and tests", icon: "queue" },
      { id: "build", x: 0, y: 416, w: 272, title: "Build and verify", sub: "9 build, 10 verify", icon: "code" },
      { id: "live", x: 0, y: 520, w: 272, title: "Cut-over", sub: "the Navree app opens", icon: "send", tone: "accent" },
    ],
    edges: [
      { from: "read.bottom", to: "base.top", label: "stop 1", step: 0 },
      { from: "base.bottom", to: "specs.top", label: "stop 2", step: 1 },
      { from: "specs.bottom", to: "plan.top", label: "stop 3", step: 2 },
      { from: "plan.bottom", to: "build.top", label: "stop 4", step: 3 },
      { from: "build.bottom", to: "live.top", label: "stop 5", step: 4 },
    ],
  },
};
