import { site } from "@site";

import { ScriptedTerminal, type TermLine } from "./scripted-terminal";

/**
 * The next steps `navree new` prints for the SaaS kit on the default
 * database: the command, padded to 30 columns, then its comment.
 */
const steps: [string, string][] = [
  ["navree dock up", "TimescaleDB and Mailpit"],
  ["navree key:generate", "then set APP_KEY in .env"],
  ["navree migrate:fresh --seed", "tables and a demo workspace"],
  ["pnpm --dir web install", "the web app's packages"],
  ["navree dev", "API on :8080, web app on :3000"],
];

/**
 * The hero's session: install the CLI, then create an app from the SaaS
 * kit. The lines after `navree new` are what the CLI prints once the app
 * is written.
 */
const session: TermLine[] = [
  { cmd: site.install.cli },
  { out: "   Installed package `navree-cli v1.0.0` (executable `navree`)" },
  { cmd: "navree new acme --kit saas", className: "mt-2" },
  { out: "Created acme in ~/code/acme (kit: saas)", className: "text-fg-2" },
  { out: "Next steps:", className: "mt-1" },
  { out: "  cd acme" },
  ...steps.map(([command, comment]): TermLine => ({
    out: (
      <>
        {"  "}
        <span className="text-fg-1">{command}</span>
        {`${" ".repeat(30 - command.length)} # ${comment}`}
      </>
    ),
  })),
];

export function HeroTerminal() {
  return <ScriptedTerminal title="~/code" lines={session} className="rounded-[calc(1.25rem-0.375rem)] border-line-2" />;
}
