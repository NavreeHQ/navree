"use client";

import { cn } from "@/lib/utils";

import { MockFrame } from "./frame";
import { harborDeploy, harborReleases } from "./mock-data";
import { useSteps } from "./use-steps";

/** Steps: the command, the heading, each release step, the result. */
export const HARBOR_STEPS = harborDeploy.length + 3;

export function useHarborSteps() {
  return useSteps(HARBOR_STEPS, { interval: (s) => (s === 0 ? 900 : s === 1 ? 1100 : 620), hold: 4200 });
}

function shown(visible: boolean) {
  return cn("transition-[opacity,translate] duration-300 ease-(--ease-out-quint)", visible ? "translate-y-0 opacity-100" : "translate-y-0.5 opacity-0");
}

/** The `navree deploy` output, line by line. Every line keeps its space, so nothing moves. */
export function DeployLog({ step, compact = false }: { step: number; compact?: boolean }) {
  const lines = Math.max(0, Math.min(harborDeploy.length, step - 1));
  const released = step >= HARBOR_STEPS - 1;
  const running = !released;
  return (
    <div className="@container font-mono text-xs leading-6 @md:text-[0.8125rem]">
      <div className="flex items-center gap-2 text-fg">
        <span className="text-fg-3">$</span>
        <span>navree deploy</span>
        {step === 0 ? <Caret /> : null}
      </div>
      <div className={cn("text-fg-2", shown(step >= 1))}>Deploying crm-api to production (1 server)</div>
      {harborDeploy.map((line, i) => {
        if (compact && (line.step === "env" || line.step === "prune" || line.step === "restart")) return null;
        const visible = i < lines;
        const current = visible && i === lines - 1 && running;
        return (
          <div
            key={line.step}
            className={cn(
              "grid gap-x-3 pl-2",
              compact ? "grid-cols-[4.25rem_minmax(0,1fr)]" : "grid-cols-[4rem_minmax(0,1fr)] @md:grid-cols-[4.25rem_minmax(0,1fr)_auto] @xl:grid-cols-[4.25rem_3rem_minmax(0,1fr)_auto]",
              shown(visible),
            )}
          >
            <span className={cn(line.step === "switch" ? "text-accent" : "text-fg-1")}>{line.step}</span>
            {compact ? null : <span className="hidden text-fg-3 @xl:block">{line.server ?? ""}</span>}
            <span className={cn("min-w-0 text-fg-2", compact ? "truncate" : "break-words")}>
              {line.detail}
              {current ? <Caret /> : null}
            </span>
            {compact ? null : <span className="hidden text-right text-fg-3 tabular-nums @md:block">{line.time ?? ""}</span>}
          </div>
        );
      })}
      <div className={cn("text-accent", shown(released))}>Released 20261026-091512-3f2a9c1 in 1m 53s.</div>
    </div>
  );
}

function Caret() {
  return <span className="ml-1 inline-block h-3.5 w-[0.45rem] translate-y-[2px] bg-fg-2 motion-safe:animate-pulse" />;
}

export type SlotState = { label: string; tone: "live" | "busy" | "idle" | "off" };

export function Slot({ name, port, release, state, active }: { name: string; port: string; release: string; state: SlotState; active: boolean }) {
  return (
    <div
      className={cn(
        "min-w-0 rounded-md border p-3 transition-[border-color,background-color] duration-500",
        active ? "border-line-2 bg-ink-2" : "border-line bg-ink-1",
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="font-mono text-[0.8125rem] text-fg">{name}</span>
        <span className="font-mono text-xs text-fg-3">:{port}</span>
      </div>
      <div className="mt-1 truncate font-mono text-xs text-fg-3">{release}</div>
      <div
        className={cn(
          "mt-2.5 inline-flex items-center gap-1.5 text-xs",
          state.tone === "live" ? "text-accent" : state.tone === "busy" ? "text-fg-1" : "text-fg-3",
        )}
      >
        <span
          className={cn(
            "size-1.5 rounded-full",
            state.tone === "live"
              ? "bg-accent"
              : state.tone === "busy"
                ? "bg-fg-1 motion-safe:animate-pulse"
                : state.tone === "off"
                  ? "bg-line-2"
                  : "bg-fg-3",
          )}
        />
        {state.label}
      </div>
    </div>
  );
}

/** Harbor: a deploy in the terminal, and the server's blue and green slots switching beside it. */
export function HarborMock({ className }: { className?: string }) {
  const { ref, step } = useHarborSteps();
  const lines = Math.max(0, Math.min(harborDeploy.length, step - 1));
  const at = (name: string) => lines > harborDeploy.findIndex((l) => l.step === name);
  const uploaded = at("upload");
  const switched = at("switch");
  const drained = at("drain");
  const restarted = at("restart");

  const green: SlotState = switched
    ? { label: "live", tone: "live" }
    : at("health")
      ? { label: "healthy · 200 in 0.4 s", tone: "busy" }
      : at("start")
        ? { label: "starting", tone: "busy" }
        : { label: "idle", tone: "idle" };
  const blue: SlotState = drained ? { label: "stopped", tone: "off" } : switched ? { label: "draining", tone: "busy" } : { label: "live", tone: "live" };

  return (
    <MockFrame
      title="~/code/crm-api"
      className={className}
      bodyClassName="@container"
      label="Harbor deploying the example CRM with navree deploy: it builds, uploads, migrates, starts the new release in the green slot, waits for GET /health/ready to answer 200, switches Caddy from blue to green, drains blue and restarts the workers. Beside the terminal, the server app-1 shows its blue and green slots and the releases kept for rollbacks."
    >
      <div ref={ref} className="grid @5xl:grid-cols-[minmax(0,1.55fr)_minmax(0,1fr)]">
        <div className="min-w-0 p-4 @xl:p-5">
          <DeployLog step={step} />
        </div>

        {/* The server: side by side with the log on wide frames, a row of three panels under it otherwise. */}
        <div className="@container min-w-0 border-t border-line bg-ink-0/30 p-4 @xl:p-5 @5xl:border-t-0 @5xl:border-l">
          <div className="grid gap-5 @2xl:grid-cols-[minmax(0,0.8fr)_minmax(0,1.2fr)_minmax(0,1fr)] @2xl:gap-6">
            <div className="min-w-0">
              <div className="flex items-center justify-between gap-2">
                <span className="text-[0.8125rem] font-semibold text-fg">app-1</span>
                <span className="font-mono text-xs text-fg-3">203.0.113.10</span>
              </div>
              <div className="mt-2 flex flex-wrap gap-1">
                {["web", "worker", "scheduler", "postgres", "redis"].map((role) => (
                  <span key={role} className="rounded-sm border border-line px-1.5 font-mono text-xs leading-5 text-fg-2">
                    {role}
                  </span>
                ))}
              </div>
              <div className="mt-3 flex items-center justify-between gap-2 rounded-md border border-line bg-ink-1 px-3 py-2 text-xs">
                <span className="text-fg-2">worker ×2 · scheduler</span>
                <span className={restarted ? "text-fg-1" : "text-fg-3"}>{restarted ? "restarted" : "running"}</span>
              </div>
            </div>

            <div className="min-w-0">
              <div className="rounded-md border border-line-2 bg-ink-2 px-3 py-2 text-center">
                <div className="font-mono text-[0.8125rem] text-fg">Caddy</div>
                <div className="font-mono text-xs text-fg-3">crm.example.com · TLS</div>
              </div>
              <div className="grid grid-cols-2 gap-3">
                {[false, true].map((isGreen) => (
                  <div key={String(isGreen)} className="flex justify-center">
                    <span
                      className={cn(
                        "h-5 w-px transition-colors duration-500",
                        (isGreen ? switched : !switched) ? "bg-accent" : "bg-line-2",
                      )}
                    />
                  </div>
                ))}
              </div>
              <div className="grid grid-cols-2 gap-3">
                <Slot name="blue" port="8081" release={harborReleases[1]!.id} state={blue} active={!switched} />
                <Slot name="green" port="8082" release={uploaded ? harborReleases[0]!.id : harborReleases[2]!.id} state={green} active={switched} />
              </div>
            </div>

            <div className="min-w-0">
              <div className="text-xs text-fg-3">Releases kept for rollbacks</div>
              <div className="mt-2 space-y-1.5">
                {harborReleases.map((r, i) => {
                  const current = switched ? i === 0 : i === 1;
                  const hidden = i === 0 && !uploaded;
                  return (
                    <div key={r.id} className={cn("flex items-center gap-2 font-mono text-xs transition-opacity duration-300", hidden && "opacity-0")}>
                      <span className={cn("size-1.5 shrink-0 rounded-full transition-colors", current ? "bg-accent" : "bg-line-2")} />
                      <span className={cn("shrink-0", current ? "text-fg" : "text-fg-3")}>{r.id.slice(-7)}</span>
                      <span className="min-w-0 truncate text-fg-3">{r.commit}</span>
                      {current ? <span className="ml-auto shrink-0 text-fg-2">current</span> : null}
                    </div>
                  );
                })}
              </div>
              <div className="mt-3 font-mono text-xs text-fg-3">
                <span className="text-fg-2">navree deploy:rollback</span> switches back
              </div>
            </div>
          </div>
        </div>
      </div>
    </MockFrame>
  );
}
