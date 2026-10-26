import type { DiagramName } from "@/components/mdx/diagram";

import { apiFirst } from "./api-first";
import { backendForFrontend } from "./backend-for-frontend";
import { eventDriven } from "./event-driven";
import type { DiagramSpec } from "./figure";
import { microservices } from "./microservices";
import { modularMonolith } from "./modular-monolith";
import { moduleGraph } from "./module-graph";
import { multiTenancy } from "./multi-tenancy";
import { requestLifecycle } from "./request-lifecycle";

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
};

/**
 * A diagram in words (its title and description), for places that can't
 * show the picture: the Markdown copies of the docs, `llms.txt`.
 */
export function diagramText(name: DiagramName): string {
  const spec = diagrams[name];
  return `${spec.title}. ${spec.desc}`;
}
