import { FleetMock } from "@/components/ecosystem/fleet-mock";
import { HarborMock } from "@/components/ecosystem/harbor-mock";
import { HelmMock } from "@/components/ecosystem/helm-mock";
import { LighthouseMock } from "@/components/ecosystem/lighthouse-mock";
import { LogbookMock } from "@/components/ecosystem/logbook-mock";

/**
 * `<ToolMock name="fleet" />`: the picture of a tool that the ecosystem page
 * shows, at the top of the tool's docs page. Each one is drawn from code, not
 * a screenshot, and says in words what it shows for screen readers.
 */
export const toolMockNames = ["fleet", "harbor", "helm", "lighthouse", "logbook"] as const;

export type ToolMockName = (typeof toolMockNames)[number];

const mocks = {
  fleet: FleetMock,
  harbor: HarborMock,
  helm: HelmMock,
  lighthouse: LighthouseMock,
  logbook: LogbookMock,
} as const;

export function ToolMock({ name }: { name: ToolMockName }) {
  const Mock = mocks[name];
  return (
    <div data-tool-mock={name} className="my-8">
      <Mock />
    </div>
  );
}
