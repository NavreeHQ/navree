"use client";

import { CompassIcon } from "lucide-react";
import { Fragment } from "react";

import { cn } from "@/lib/utils";

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
      <code key={i} className="rounded-[5px] border border-line bg-ink-2 px-1 py-px font-mono text-[0.8em] text-fg">
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
            <div key={i} className={cn("rounded-lg border border-line bg-ink-2 px-3 py-2 text-[0.8125rem] leading-relaxed text-fg", fade(visible))}>
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
                    done ? "bg-accent" : "bg-fg-1 motion-safe:animate-pulse",
                  )}
                />
                <span className="min-w-0 truncate">
                  <span className="text-fg-3">navree · </span>
                  <span className="text-fg">{item.tool}</span>
                  <span className="text-fg-2">({item.args ?? ""})</span>
                </span>
              </div>
              {compact ? null : (
                <div className={cn("mt-0.5 grid grid-cols-[1.25rem_minmax(0,1fr)] text-fg-2", fade(done))}>
                  <span className="text-fg-3">⎿</span>
                  <div className="min-w-0">
                    {item.result.map((line) => (
                      <div key={line} className="break-words">
                        {line}
                      </div>
                    ))}
                  </div>
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

  return (
    <MockFrame
      title="agent · ~/code/crm"
      className={className}
      bodyClassName="@container"
      label="A coding agent fixing the example CRM with Navigator. Asked why the deals page returns 500, it calls last_error, which shows a missing column deals.expected_close_date; describe_schema, which shows the migration that adds it is pending; and list_commands. It answers: run navree migrate. Beside the session, Navigator's twelve tools, with the three it used lit."
    >
      <div ref={ref} className="grid @3xl:grid-cols-[minmax(0,1.55fr)_minmax(0,1fr)]">
        <div className="min-w-0 p-4 @xl:p-5">
          <AgentSession step={step} />
        </div>
        <div className="min-w-0 border-t border-line bg-ink-0/30 p-4 @xl:p-5 @3xl:border-t-0 @3xl:border-l">
          <div className="flex items-center gap-2.5">
            <CompassIcon className="size-4 text-accent" strokeWidth={1.5} />
            <span className="text-[0.8125rem] font-semibold text-fg">navree</span>
            <span className="ml-auto font-mono text-xs text-fg-3">stdio · 12 tools</span>
          </div>
          <div className="mt-1 font-mono text-xs text-fg-3">navree mcp · read-only</div>
          <div className="mt-4 grid grid-cols-2 gap-1.5">
            {navigatorTools.map((tool) => {
              const s = state(tool);
              return (
                <span
                  key={tool}
                  className={cn(
                    "truncate rounded-md border px-2 py-1 font-mono text-xs transition-[color,border-color,background-color,box-shadow] duration-300",
                    s === "active"
                      ? "border-accent/60 bg-accent/10 text-accent shadow-glow-sm"
                      : s === "done"
                        ? "border-line-2 bg-ink-2 text-fg"
                        : "border-line text-fg-3",
                  )}
                >
                  {tool}
                </span>
              );
            })}
          </div>
          <div className="mt-4 rounded-lg border border-line bg-ink-1 px-3 py-2 font-mono text-xs leading-5 text-fg-3">
            <div>
              navree <span className="text-fg-2">0.1</span> · rust <span className="text-fg-2">1.95</span>
            </div>
            <div>database sqlite · local only</div>
          </div>
        </div>
      </div>
    </MockFrame>
  );
}
