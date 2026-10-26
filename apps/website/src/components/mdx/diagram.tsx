import { DiagramFigure, diagrams } from "@/components/diagrams";

/**
 * `<Diagram name="microservices" />`: an architecture diagram from
 * `src/components/diagrams/`, framed with its title and legend. Each one is
 * hand-drawn SVG with a title and a description for screen readers, and
 * re-lays out vertically in narrow columns and on phones.
 */
export const diagramNames = [
  "modular-monolith",
  "microservices",
  "multi-tenancy",
  "event-driven",
  "api-first",
  "backend-for-frontend",
  "request-lifecycle",
  "module-graph",
] as const;

export type DiagramName = (typeof diagramNames)[number];

export function Diagram({ name }: { name: DiagramName }) {
  return <DiagramFigure spec={diagrams[name]} name={name} className="my-8" />;
}
