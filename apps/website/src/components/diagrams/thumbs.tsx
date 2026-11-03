import type { ReactNode } from "react";

import type { DiagramName } from "@/components/mdx/diagram";
import { cn } from "@/lib/utils";

/*
 * Small pictures of each architecture for indexes and tabs, drawn on a
 * 160 x 88 canvas with the same vocabulary as the full diagrams: ink boxes
 * with line-2 strokes, one green element, dashed lines for events. They
 * are decoration (aria-hidden): the link or tab next to them names them.
 */

const box = "fill-ink-2 stroke-line-2";
const green = "fill-accent/10 stroke-accent/80";
const line = "stroke-bar";

function R({ x, y, w, h, accent = false, r = 5 }: { x: number; y: number; w: number; h: number; accent?: boolean; r?: number }) {
  return <rect x={x + 0.5} y={y + 0.5} width={w - 1} height={h - 1} rx={r} className={accent ? green : box} />;
}

function L({ d, dashed = false, className }: { d: string; dashed?: boolean; className?: string }) {
  return (
    <path
      d={d}
      fill="none"
      strokeWidth={1.25}
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeDasharray={dashed ? "3 3" : undefined}
      className={cn(line, className)}
    />
  );
}

/** Three short text lines inside a box, as if it had content. */
function Lines({ x, y, w, accent = false }: { x: number; y: number; w: number; accent?: boolean }) {
  return (
    <g className={accent ? "stroke-accent/60" : "stroke-fg-3/40"} strokeWidth={1.5} strokeLinecap="round">
      <path d={`M${x} ${y}h${w}`} />
      <path d={`M${x} ${y + 6}h${w * 0.7}`} />
      <path d={`M${x} ${y + 12}h${w * 0.85}`} />
    </g>
  );
}

const thumbs: Record<Exclude<DiagramName, "request-lifecycle" | "module-graph" | "navree-ui">, ReactNode> = {
  "modular-monolith": (
    <>
      <rect x={20.5} y={8.5} width={119} height={71} rx={10} className="fill-fg/[0.015] stroke-line-2" />
      <R x={32} y={20} w={44} h={22} />
      <R x={84} y={20} w={44} h={22} accent />
      <R x={32} y={48} w={44} h={22} />
      <R x={84} y={48} w={44} h={22} />
      <L d="M76 31h8" />
      <L d="M54 42v6" />
      <L d="M106 42v6" />
    </>
  ),
  microservices: (
    <>
      <R x={14} y={8} w={38} h={26} />
      <R x={61} y={8} w={38} h={26} />
      <R x={108} y={8} w={38} h={26} />
      <L d="M33 34v20 M80 34v20 M127 34v20" dashed />
      <rect x={14.5} y={54.5} width={131} height={13} rx={6.5} className={green} />
      <g className="fill-fg-3/50">
        <circle cx={33} cy={77} r={2} />
        <circle cx={80} cy={77} r={2} />
        <circle cx={127} cy={77} r={2} />
      </g>
    </>
  ),
  "multi-tenancy": (
    <>
      <R x={52} y={8} w={88} h={48} r={7} />
      <R x={38} y={20} w={88} h={48} r={7} />
      <R x={24} y={32} w={88} h={48} r={7} accent />
      <Lines x={36} y={48} w={40} accent />
    </>
  ),
  "event-driven": (
    <>
      <R x={14} y={30} w={44} h={28} accent />
      <path d="M38 37.5 31.5 46H36l-1.5 6 6.5-8.5H36.5l1.5-6Z" className="fill-accent/80" />
      <L d="M58 44H82" />
      <L d="M82 44V18H104 M82 44H104 M82 44V70H104" dashed />
      <R x={104} y={8} w={42} h={20} />
      <R x={104} y={34} w={42} h={20} />
      <R x={104} y={60} w={42} h={20} />
    </>
  ),
  "api-first": (
    <>
      <path d="M22.5 12.5h26l10 10v52a3 3 0 0 1-3 3h-33a3 3 0 0 1-3-3v-59a3 3 0 0 1 3-3Z" className={green} />
      <path d="M48.5 12.5v10h10" fill="none" className="stroke-accent/80" />
      <Lines x={27} y={34} w={24} accent />
      <L d="M59 45H82 M82 45V19H104 M82 45V71H104 M82 45H104" />
      <R x={104} y={9} w={42} h={20} />
      <R x={104} y={35} w={42} h={20} />
      <R x={104} y={61} w={42} h={20} />
    </>
  ),
  "backend-for-frontend": (
    <>
      <R x={8} y={30} w={38} h={28} />
      <path d="M8.5 37.5h37" className="stroke-line-2" />
      <R x={61} y={24} w={38} h={40} accent />
      <path d="M74.5 44.5h11v9h-11z M76.75 44.5v-2.5a3.25 3.25 0 0 1 6.5 0v2.5" fill="none" className="stroke-accent" strokeWidth={1.25} />
      <R x={114} y={30} w={38} h={28} />
      <L d="M46 44h15 M99 44h15" />
    </>
  ),
};

export type ThumbName = keyof typeof thumbs;

export function PatternThumb({ name, className }: { name: ThumbName; className?: string }) {
  return (
    <svg viewBox="0 0 160 88" aria-hidden="true" className={cn("block h-auto w-full", className)}>
      {thumbs[name]}
    </svg>
  );
}

/**
 * The three stages of "grow without rewriting": one binary; the same
 * binary serving many workspaces; the deals module moved into a service.
 * The deals module is the green one in each.
 */
export function GrowThumb({ stage, className }: { stage: 1 | 2 | 3; className?: string }) {
  return (
    <svg viewBox="0 0 200 96" aria-hidden="true" className={cn("block h-auto w-full", className)}>
      {stage === 2 ? (
        <>
          <rect x={44.5} y={4.5} width={119} height={79} rx={10} className="fill-ink-1 stroke-line-2" />
          <rect x={36.5} y={8.5} width={119} height={79} rx={10} className="fill-ink-1 stroke-line-2" />
        </>
      ) : null}
      <rect
        x={stage === 3 ? 12.5 : 28.5}
        y={12.5}
        width={119}
        height={79}
        rx={10}
        className={cn(stage === 2 ? "fill-ink-1" : "fill-fg/[0.015]", "stroke-line-2")}
      />
      {stage === 3 ? (
        <>
          <R x={24} y={26} w={44} h={22} />
          <R x={76} y={26} w={44} h={22} />
          <R x={24} y={56} w={44} h={22} />
          <rect x={76.5} y={56.5} width={43} height={21} rx={5} strokeDasharray="3 3" className="fill-none stroke-line-2" />
          <L d="M120 67H148" dashed />
          <rect x={148.5} y={44.5} width={43} height={45} rx={9} className="fill-fg/[0.015] stroke-line-2" />
          <R x={156} y={56} w={28} h={22} accent />
        </>
      ) : (
        <>
          <R x={40} y={26} w={44} h={22} />
          <R x={92} y={26} w={44} h={22} />
          <R x={40} y={56} w={44} h={22} />
          <R x={92} y={56} w={44} h={22} accent />
        </>
      )}
    </svg>
  );
}
