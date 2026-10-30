"use client";

import { CheckIcon, CompassIcon, LoaderCircleIcon } from "lucide-react";
import { Fragment } from "react";

import { cn } from "@/lib/utils";

import { LogoTile, StatusDot } from "./app-shell";
import { MockFrame } from "./frame";
import { navigatorSession, navigatorTools } from "./mock-data";
import { useSteps } from "./use-steps";

type Item = (typeof navigatorSession)[number];

/**
 * Each tool call takes two steps (the call, then its result), so the
 * session's steps are: prompt, call/result × 3, answer.
 */
type Beat = { item: number; part: "call" | "result" | "whole" };
const timeline: Beat[] = navigatorSession.flatMap((item, i): Beat[] =>
  item.type === "call" ? [{ item: i, part: "call" }, { item: i, part: "result" }] : [{ item: i, part: "whole" }],
);

export function useNavigatorSteps() {
  return useSteps(timeline.length, { interval: (s) => (s === 0 ? 1400 : timeline[s]?.part === "call" ? 900 : 1100), hold: 4200 });
}

/** How far the session has got at `step`: which items show, and how much of each. */
function progress(step: number) {
  const seen = timeline.slice(0, step + 1);
  const shownItems = new Set(seen.map((t) => t.item));
  const resulted = new Set(seen.filter((t) => t.part === "result").map((t) => t.item));
  return { shownItems, resulted };
}

function InlineCode({ text }: { text: string }) {
  const parts = text.split(/(`[^`]+`)/);
  return parts.map((p, i) =>
    p.startsWith("`") ? (
      <code key={i} className="rounded-sm border border-line bg-ink-2 px-1 py-px font-mono text-[0.8em] text-fg">
        {p.slice(1, -1)}
      </code>
    ) : (
      <Fragment key={i}>{p}</Fragment>
    ),
  );
}

function fade(visible: boolean) {
  return cn("transition-[opacity,translate] duration-300 ease-(--ease-out-quint)", visible ? "translate-y-0 opacity-100" : "translate-y-1 opacity-0");
}

/** The agent's side: the prompt, Navigator's tool calls with their results, and the answer. */
export function AgentSession({ step, compact = false }: { step: number; compact?: boolean }) {
  const { shownItems, resulted } = progress(step);
  return (
    <div className="space-y-3">
      {navigatorSession.map((item: Item, i) => {
        const visible = shownItems.has(i);
        if (item.type === "prompt") {
          return (
            <div key={i} className={cn("rounded-md border border-line-2 bg-ink-2 px-3 py-2 text-[0.8125rem] leading-relaxed text-fg", fade(visible))}>
              <span className="mr-2 font-mono text-fg-3">&gt;</span>
              {item.text}
            </div>
          );
        }
        if (item.type === "call") {
          const done = resulted.has(i);
          return (
            <div key={i} className={cn("font-mono text-xs leading-5", fade(visible))}>
              <div className="flex items-center gap-2">
                <span
                  className={cn(
                    "size-1.5 shrink-0 rounded-full transition-colors duration-300",
                    done ? "bg-fg-3" : "bg-fg-1 motion-safe:animate-pulse",
                  )}
                />
                <span className="min-w-0 truncate">
                  <span className="text-fg-3">navree · </span>
                  <span className="text-fg">{item.tool}</span>
                  <span className="text-fg-2">({item.args ?? ""})</span>
                </span>
              </div>
              {compact ? null : (
                <div className={cn("mt-0.5 ml-[0.1875rem] border-l border-line-2 pl-2.5 text-fg-2", fade(done))}>
                  {item.result.map((line) => (
                    <div key={line} className="[overflow-wrap:anywhere]">
                      {line}
                    </div>
                  ))}
                </div>
              )}
            </div>
          );
        }
        // Compact sessions keep only the answer's last sentence: what to do.
        const text = compact ? (item.text.split(/(?<=\.)\s+/).pop() ?? item.text) : item.text;
        return (
          <p key={i} className={cn("text-[0.8125rem] leading-relaxed text-pretty text-fg-1", fade(visible))}>
            <InlineCode text={text} />
          </p>
        );
      })}
    </div>
  );
}

/** Navigator: a coding agent's session calling `navree mcp`, and the server's tools lighting up as they are used. */
export function NavigatorMock({ className }: { className?: string }) {
  const { ref, step } = useNavigatorSteps();
  const { shownItems, resulted } = progress(step);
  const calls = navigatorSession.flatMap((item, i) => (item.type === "call" ? [{ tool: item.tool, i }] : []));
  const state = (tool: string) => {
    const call = calls.find((c) => c.tool === tool);
    if (!call || !shownItems.has(call.i)) return "idle";
    return resulted.has(call.i) ? "done" : "active";
  };
  const used = calls.filter((c) => resulted.has(c.i)).length;

  return (
    <MockFrame
      title="agent · ~/code/crm"
      className={className}
      bodyClassName="@container"
      label="A coding agent fixing the example CRM with Navigator. Asked why the deals page returns 500, it calls last_error, which shows a missing column deals.expected_close_date; describe_schema, which shows the migration that adds it is pending; and list_commands. It answers: run navree migrate. Beside the session, Navigator's twelve tools, with the three it used marked."
    >
      <div ref={ref} className="grid h-full @3xl:grid-cols-[minmax(0,1.55fr)_minmax(0,1fr)]">
        <div className="min-w-0 p-4 @xl:p-5">
          <AgentSession step={step} />
        </div>
        {/* The MCP server, as the agent's client lists it. */}
        <div className="flex min-w-0 flex-col border-t border-line bg-ink-0 @3xl:border-t-0 @3xl:border-l">
          <div className="flex items-center gap-2.5 border-b border-line p-3 @xl:px-4">
            <LogoTile tone="outline">
              <CompassIcon strokeWidth={1.75} />
            </LogoTile>
            <div className="grid min-w-0 flex-1 leading-tight">
              <span className="truncate text-sm font-medium text-fg">navree</span>
              <span className="truncate font-mono text-xs text-fg-3">navree mcp · stdio</span>
            </div>
            <span className="inline-flex shrink-0 items-center gap-2 text-xs text-fg-2">
              <StatusDot />
              Connected
            </span>
          </div>
          <div className="p-2 @xl:px-3">
            <div className="flex h-8 items-center justify-between px-2 text-xs text-fg-3">
              <span>Tools</span>
              <span className="font-mono tabular-nums">
                {used} of {navigatorTools.length} used
              </span>
            </div>
            <div className="grid grid-cols-2 gap-x-1 gap-y-0.5">
              {navigatorTools.map((tool) => {
                const s = state(tool);
                return (
                  <span
                    key={tool}
                    className={cn(
                      "flex h-7 min-w-0 items-center gap-2 rounded-sm px-2 font-mono text-xs transition-colors duration-300",
                      s === "active" ? "bg-ink-3 text-fg" : s === "done" ? "bg-ink-2 text-fg-1" : "text-fg-3",
                    )}
                  >
                    <span className="min-w-0 flex-1 truncate">{tool}</span>
                    {s === "active" ? (
                      <LoaderCircleIcon className="size-3.5 shrink-0 text-fg-2 motion-safe:animate-spin" strokeWidth={2} />
                    ) : s === "done" ? (
                      <CheckIcon className="size-3.5 shrink-0 text-fg-2" strokeWidth={2} />
                    ) : null}
                  </span>
                );
              })}
            </div>
          </div>
          <div className="mt-auto border-t border-line px-4 py-3 text-xs leading-5 text-fg-3">
            <div className="flex justify-between gap-3">
              <span>Version</span>
              <span className="font-mono text-fg-2">navree 0.2 · rust 1.95</span>
            </div>
            <div className="flex justify-between gap-3">
              <span>Database</span>
              <span className="font-mono text-fg-2">sqlite · local only</span>
            </div>
            <div className="flex justify-between gap-3">
              <span>Access</span>
              <span className="font-mono text-fg-2">read-only</span>
            </div>
          </div>
        </div>
      </div>
    </MockFrame>
  );
}
