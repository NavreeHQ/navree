import type { DiagramName } from "@/components/mdx/diagram";

import { apiFirst } from "./api-first";
import { backendForFrontend } from "./backend-for-frontend";
import { designToUi } from "./design-to-ui";
import { eventDriven } from "./event-driven";
import type { DiagramSpec } from "./figure";
import { harborRelease } from "./harbor-release";
import { jobsAndEvents } from "./jobs-and-events";
import { microservices } from "./microservices";
import { modularMonolith } from "./modular-monolith";
import { moduleGraph } from "./module-graph";
import { multiTenancy } from "./multi-tenancy";
import { navreeUi } from "./navree-ui";
import { ormParts } from "./orm-parts";
import { requestLifecycle } from "./request-lifecycle";
import { reverseEngineering } from "./reverse-engineering";
import { transportChoice } from "./transport-choice";
import { twoWays } from "./two-ways";
import { workspaceIsolation } from "./workspace-isolation";
import { workspaceResolution } from "./workspace-resolution";

export { DiagramCanvas, DiagramFigure, DiagramLegend, type DiagramSpec } from "./figure";

/** Every diagram by its `<Diagram name>`. */
export const diagrams: Record<DiagramName, DiagramSpec> = {
  "modular-monolith": modularMonolith,
  microservices,
  "multi-tenancy": multiTenancy,
  "event-driven": eventDriven,
  "api-first": apiFirst,
  "backend-for-frontend": backendForFrontend,
  "request-lifecycle": requestLifecycle,
  "module-graph": moduleGraph,
  "navree-ui": navreeUi,
  "reverse-engineering": reverseEngineering,
  "design-to-ui": designToUi,
  "two-ways": twoWays,
  "workspace-resolution": workspaceResolution,
  "workspace-isolation": workspaceIsolation,
  "harbor-release": harborRelease,
  "orm-parts": ormParts,
  "jobs-and-events": jobsAndEvents,
  "transport-choice": transportChoice,
};

/**
 * A diagram in words (its title and description), for places that can't
 * show the picture: the Markdown copies of the docs, `llms.txt`.
 */
export function diagramText(name: DiagramName): string {
  const spec = diagrams[name];
  return `${spec.title}. ${spec.desc}`;
}
