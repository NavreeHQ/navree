import { site } from "@site";

import { ScriptedTerminal, type TermLine } from "./scripted-terminal";

/**
 * The next steps `navree new` prints for the SaaS kit on the default
 * database: the command in a 30-column field, then its comment. Where the
 * terminal is too narrow for the longest step (2 + 30 + 1 + 32 columns, about
 * 32rem at this size), every comment goes under its command, so the steps
 * line up either way.
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
  // Indented three columns, as cargo prints it; a wrapped line continues two further in.
  { out: "Installed package `navree-cli v1.0.0` (executable `navree`)", className: "pl-[5ch] -indent-[2ch]" },
  { cmd: "navree new cloudworks --kit saas", className: "mt-2" },
  { out: "Created cloudworks in ~/code/cloudworks (kit: saas)", className: "text-fg-2" },
  { out: "Next steps:", className: "mt-1" },
  { out: "  cd cloudworks" },
  ...steps.map(([command, comment]): TermLine => ({
    out: (
      <span className="flex flex-col pl-[2ch] @min-[32rem]:flex-row @min-[32rem]:gap-x-[1ch]">
        <span className="text-fg-1 @min-[32rem]:w-[30ch] @min-[32rem]:shrink-0">{command}</span>
        <span className="min-w-0"># {comment}</span>
      </span>
    ),
  })),
];

export function HeroTerminal() {
  return <ScriptedTerminal title="~/code" lines={session} className="rounded-[calc(1.25rem-0.375rem)] border-line-2" />;
}
