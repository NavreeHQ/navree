import type { ReactNode } from "react";

import { ArrowLink, HomeSection, SectionHeading } from "./parts";
import { WorkspaceDemo } from "./workspace-demo";

/* Small glyphs for the three isolation strategies: the green part is "this workspace". */
function RowGlyph() {
  return (
    <svg viewBox="0 0 72 48" aria-hidden className="h-12 w-[4.5rem]">
      <rect x="0.5" y="0.5" width="71" height="47" rx="6" className="fill-ink-1 stroke-line-2" />
      <path d="M0.5 12.5H71.5" className="stroke-line-2" />
      {[0, 1, 2].map((i) => (
        <path key={i} d={`M0.5 ${21.5 + i * 9}H71.5`} className="stroke-line" />
      ))}
      <rect x="6" y="15" width="10" height="4" rx="1" className="fill-accent" />
      <rect x="20" y="15" width="40" height="4" rx="1" className="fill-accent/35" />
      <rect x="6" y="33" width="10" height="4" rx="1" className="fill-accent" />
      <rect x="20" y="33" width="34" height="4" rx="1" className="fill-accent/35" />
      <rect x="6" y="24" width="10" height="4" rx="1" className="fill-line-2" />
      <rect x="20" y="24" width="44" height="4" rx="1" className="fill-line-2" />
      <rect x="6" y="42" width="10" height="3" rx="1" className="fill-line-2" />
      <rect x="20" y="42" width="30" height="3" rx="1" className="fill-line-2" />
    </svg>
  );
}

function Cylinder({ x, live }: { x: number; live?: boolean }) {
  const cls = live ? "fill-accent/10 stroke-accent" : "fill-ink-1 stroke-line-2";
  return (
    <g>
      <path d={`M${x} 10v28c0 2.8 4.5 5 10 5s10-2.2 10-5V10`} className={cls} />
      <ellipse cx={x + 10} cy="10" rx="10" ry="5" className={cls} />
    </g>
  );
}

function SchemaGlyph() {
  return (
    <svg viewBox="0 0 72 48" aria-hidden className="h-12 w-[4.5rem]">
      <path d="M16 8v32c0 3.9 9 7 20 7s20-3.1 20-7V8" className="fill-ink-1 stroke-line-2" />
      <ellipse cx="36" cy="8" rx="20" ry="7" className="fill-ink-1 stroke-line-2" />
      <rect x="24" y="19" width="24" height="6" rx="1.5" className="fill-accent/20 stroke-accent" />
      <rect x="24" y="28" width="24" height="6" rx="1.5" className="fill-ink-2 stroke-line-2" />
      <rect x="24" y="37" width="24" height="5" rx="1.5" className="fill-ink-2 stroke-line-2" />
    </svg>
  );
}

function DatabaseGlyph() {
  return (
    <svg viewBox="0 0 72 48" aria-hidden className="h-12 w-[4.5rem]">
      <Cylinder x={1} live />
      <Cylinder x={26} />
      <Cylinder x={51} />
    </svg>
  );
}

const isolation: { name: string; code: string; glyph: ReactNode; text: string }[] = [
  {
    name: "Row",
    code: 'isolation = "row"',
    glyph: <RowGlyph />,
    text: "One set of tables with a workspace_id column. The model scopes every query to the current workspace, and PostgreSQL row-level security enforces it in the database.",
  },
  {
    name: "Schema",
    code: 'isolation = "schema"',
    glyph: <SchemaGlyph />,
    text: "One PostgreSQL schema per workspace. Each request's transaction sets its search path, and migrations run in every schema.",
  },
  {
    name: "Database",
    code: 'isolation = "database"',
    glyph: <DatabaseGlyph />,
    text: "One database per workspace, for the strictest separation. The unit of work opens on the workspace's own database.",
  },
];

/** "Multi-tenant from the first commit": the workspace spotlight. */
export function Workspaces() {
  return (
    <HomeSection id="workspaces">
      <SectionHeading
        split
        id="workspaces"
        eyebrow="Workspaces"
        title={
          <>
            <span className="lg:block">Multi-tenant from </span>the first commit.
          </>
        }
        lead={
          <p>
            Workspaces are a core module, on by default. Members, roles (<span className="text-fg-1">Owner</span>,{" "}
            <span className="text-fg-1">Admin</span>, <span className="text-fg-1">Member</span>,{" "}
            <span className="text-fg-1">Viewer</span> and your own), invitations by email and workspace-scoped permissions come
            with it. Pick a workspace below.
          </p>
        }
        action={<ArrowLink href="/docs/workspaces/introduction/">Read about workspaces</ArrowLink>}
      />

      <div className="mt-12 lg:mt-14">
        <WorkspaceDemo />
      </div>

      <div className="mt-3 grid overflow-hidden rounded-[1.25rem] border border-line bg-ink-1 shadow-bezel md:grid-cols-3">
        {isolation.map((item, i) => (
          <div key={item.name} className={`flex flex-col p-6 sm:p-7 ${i > 0 ? "border-line max-md:border-t md:border-l" : ""}`}>
            <div className="flex items-start justify-between gap-4">
              <div>
                <p className="font-mono text-xs text-fg-3">Isolation</p>
                <h3 className="mt-1.5 font-display text-display-sm font-semibold text-fg">{item.name}</h3>
              </div>
              {item.glyph}
            </div>
            <p className="mt-4 mb-5 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{item.text}</p>
            <code className="mt-auto w-fit rounded-md border border-line bg-ink-0 px-2 py-1 font-mono text-xs text-fg-1">{item.code}</code>
          </div>
        ))}
      </div>
    </HomeSection>
  );
}
