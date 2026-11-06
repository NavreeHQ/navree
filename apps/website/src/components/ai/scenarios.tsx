import type { ReactNode } from "react";

/*
 * The agent sessions the site shows: what the user asked, the skills the
 * agent loads, the Navigator tools it calls with their results, the commands
 * it runs and its answer. The animated session (`agent-session.tsx`) plays
 * them line by line, and the still session on /ai/ lays the first one out as
 * steps. One list, so the two cannot disagree. The tool names and the shape of
 * their results follow /docs/ai/navigator/; the app is a CRM.
 */

export type Line =
  | { kind: "prompt"; text: string }
  | { kind: "skill"; name: string }
  | { kind: "tool"; name: string; args?: string; result: ReactNode[] }
  | { kind: "run"; verb: string; detail: string; result?: ReactNode[] }
  | { kind: "reply"; text: ReactNode };

export type Scenario = { id: string; label: string; lines: Line[] };

const C = ({ children }: { children: ReactNode }) => <span className="font-mono text-[0.92em] text-fg-1">{children}</span>;

export const scenarios: Scenario[] = [
  {
    id: "debug",
    label: "Fix a 500",
    lines: [
      { kind: "prompt", text: "The deals page is empty since I pulled main, and the API returns 500. Fix it." },
      {
        kind: "tool",
        name: "last_error",
        result: [
          <>
            <span className="text-danger">500</span> GET /api/v1/deals <span className="text-fg-3">request_id=0199c4b1</span>
          </>,
          <>error: no such column: deals.expected_close_date</>,
        ],
      },
      {
        kind: "tool",
        name: "describe_schema",
        args: 'table: "deals"',
        result: [
          <>deals: id, title, amount_cents, currency, stage, position, …</>,
          <>
            pending migration: <C>m20261026_000004_add_expected_close_date</C>
          </>,
        ],
      },
      { kind: "run", verb: "Run", detail: "navree migrate", result: [<>Applied m20261026_000004_add_expected_close_date</>] },
      {
        kind: "run",
        verb: "Run",
        detail: "curl -i localhost:8080/api/v1/deals",
        result: [
          <>
            <span className="text-accent">200</span> GET /api/v1/deals <span className="text-fg-3">ok</span>
          </>,
        ],
      },
      {
        kind: "reply",
        text: (
          <>
            The code you pulled reads <C>deals.expected_close_date</C>, but the migration that adds it hadn&rsquo;t run on
            your database. I ran <C>navree migrate</C>; <C>GET /api/v1/deals</C> answers 200 again.
          </>
        ),
      },
    ],
  },
  {
    id: "route",
    label: "Add a route",
    lines: [
      { kind: "prompt", text: "Let a manager reassign a deal to another rep, and test it." },
      { kind: "skill", name: "navree-controllers-and-routing" },
      { kind: "skill", name: "navree-access-control" },
      {
        kind: "tool",
        name: "list_routes",
        args: 'path: "/api/v1/deals"',
        result: [
          <>
            GET&nbsp;&nbsp;/api/v1/deals <span className="text-fg-3">can:read Deal</span>
          </>,
          <>
            POST /api/v1/deals/{"{id}"}/move <span className="text-fg-3">can:move Deal from id</span>
          </>,
        ],
      },
      {
        kind: "tool",
        name: "module_graph",
        args: 'module: "DealsModule"',
        result: [<>DealsModule exports DealService</>],
      },
      {
        kind: "run",
        verb: "Edit",
        detail: "crates/crm-deals/src/controller.rs",
        result: [
          <>
            <span className="text-accent">+</span> #[post(&quot;/{"{id}"}/reassign&quot;)] #[can(Manage, Deal, from = &quot;id&quot;)]
          </>,
        ],
      },
      {
        kind: "run",
        verb: "Run",
        detail: "cargo test -p crm-deals reassign",
        result: [
          <>
            2 passed: manager_reassigns_deal, rep_cannot_reassign <span className="text-accent">ok</span>
          </>,
        ],
      },
      {
        kind: "reply",
        text: (
          <>
            Added <C>POST /api/v1/deals/{"{id}"}/reassign</C> next to <C>move</C>, behind <C>Manage</C> on the deal, with a
            validated body and two tests.
          </>
        ),
      },
    ],
  },
];
