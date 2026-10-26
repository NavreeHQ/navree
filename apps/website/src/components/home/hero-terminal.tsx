import { ScriptedTerminal, type TermLine } from "./scripted-terminal";

/**
 * The hero's session: install the CLI, create an app from the SaaS kit,
 * start it. The output follows the CLI (`navree dev`, the HTTP module's
 * "listening on" line) and the starter-kit docs.
 */
const session: TermLine[] = [
  { cmd: "cargo install navree-cli" },
  { out: "  Installed navree-cli v0.1.0" },
  { cmd: "navree new acme --kit saas", className: "mt-2" },
  { out: "Created acme in ~/code/acme (kit: saas)", className: "text-fg-2" },
  {
    out: (
      <>
        {"  "}
        <span className="text-fg-1">src/</span>
        {"       auth, workspaces, billing"}
      </>
    ),
  },
  {
    out: (
      <>
        {"  "}
        <span className="text-fg-1">web/</span>
        {"       Next.js and shadcn/ui"}
      </>
    ),
  },
  {
    out: (
      <>
        {"  "}
        <span className="text-fg-1">AGENTS.md</span>
        {"  and the agent skills"}
      </>
    ),
  },
  { cmd: "cd acme && navree dev", className: "mt-2" },
  { out: "[navree dev] building" },
  { out: "listening on http://127.0.0.1:8080" },
  { out: "✓ API on :8080, web app on :3000", className: "text-accent" },
];

export function HeroTerminal() {
  return <ScriptedTerminal title="~/code" lines={session} className="rounded-[calc(1.25rem-0.375rem)] border-line-2" />;
}
