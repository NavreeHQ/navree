import type { CSSProperties, ReactNode } from "react";

import { cn } from "@/lib/utils";

import { Glyph, type GlyphName } from "./glyphs";

/*
 * The diagram kit. A diagram is data (nodes, groups, edges and notes on an
 * 8 px grid) and this file draws it as SVG, so every diagram follows the
 * same rules:
 *
 * - Lines start and end at the centre of a side (`"node.bottom"`), and are
 *   routed with right angles and rounded corners. An arrowhead shows the
 *   direction.
 * - Text is never smaller than 12 px at 1:1. The frame (figure.tsx) shows
 *   the wide layout only when it fits at (almost) 1:1, and a vertical
 *   layout below that.
 * - One node or group carries the accent; everything else is ink and line.
 * - Lines with a `step` carry a pulse of light. Steps run in order and the
 *   kit times them from the lines' real lengths, so a request reads as one
 *   movement through the picture.
 */

export type Side = "top" | "right" | "bottom" | "left";
export type Pt = readonly [number, number];
export type Tone = "default" | "accent" | "muted";
export type EdgeKind = "flow" | "async" | "dep";

export type DiagramNode = {
  id: string;
  x: number;
  y: number;
  w: number;
  /** 56 by default (a title and a sub-line); 40 for a title alone. */
  h?: number;
  title: string;
  /** Set the title in mono: type, crate and command names. */
  mono?: boolean;
  sub?: string;
  /** Set the sub-line in mono. */
  subMono?: boolean;
  icon?: GlyphName;
  tone?: Tone;
  /** Center the text (nodes without an icon are centred by default). */
  align?: "start" | "center";
  /**
   * Rows drawn as inset chips below the title: a mono name, or a pair of a
   * mono name and a plain-text note (right-aligned).
   */
  rows?: (string | readonly [string, string])[];
};

export type DiagramGroup = {
  id?: string;
  x: number;
  y: number;
  w: number;
  h: number;
  label: string;
  dashed?: boolean;
  tone?: Tone;
};

export type DiagramEdge = {
  /** `"node.side"` or `"group.side"`. */
  from: string;
  to: string;
  kind?: EdgeKind;
  /** Corner points between the two ends, when the automatic route won't do. */
  via?: Pt[];
  /** Where the middle segment of an automatic elbow runs (x or y). */
  bend?: number;
  label?: string;
  labelMono?: boolean;
  /** The label's anchor point (baseline) and alignment. */
  at?: Pt;
  anchor?: "start" | "middle" | "end";
  /** Lines with a step carry a pulse; steps run in order. */
  step?: number;
  arrow?: "end" | "start" | "both" | "none";
};

export type DiagramNote = {
  x: number;
  y: number;
  text: string;
  anchor?: "start" | "middle" | "end";
  mono?: boolean;
  tone?: "fg-1" | "fg-2" | "fg-3" | "accent";
  size?: 12 | 13 | 14;
};

export type DiagramLayout = {
  /** The drawing area, in px at 1:1, without the outer margin. */
  width: number;
  height: number;
  groups?: DiagramGroup[];
  nodes: DiagramNode[];
  edges: DiagramEdge[];
  notes?: DiagramNote[];
  /** Hand-drawn extras (a trace waterfall), in the same coordinates. */
  extras?: ReactNode;
};

/** The outer margin around every drawing, so nothing touches the frame. */
export const MARGIN = 16;
const ARROW = 7;
const RADIUS = 8;
/** Pulse speed, px per second, and the rest between two runs of a flow. */
const SPEED = 190;
const REST = 1.4;
const GAP = 0.08;

type Box = { x: number; y: number; w: number; h: number };

function nodeHeight(n: DiagramNode): number {
  if (n.h !== undefined) return n.h;
  return n.sub ? 56 : 40;
}

function anchorOf(box: Box, side: Side): Pt {
  switch (side) {
    case "top":
      return [box.x + box.w / 2, box.y];
    case "bottom":
      return [box.x + box.w / 2, box.y + box.h];
    case "left":
      return [box.x, box.y + box.h / 2];
    case "right":
      return [box.x + box.w, box.y + box.h / 2];
  }
}

function parseEnd(ref: string, boxes: Map<string, Box>): { pt: Pt; side: Side; id: string } {
  const dot = ref.lastIndexOf(".");
  const id = ref.slice(0, dot);
  const side = ref.slice(dot + 1) as Side;
  const box = boxes.get(id);
  if (!box) throw new Error(`diagram: no node or group "${id}"`);
  return { pt: anchorOf(box, side), side, id };
}

const vertical = (s: Side) => s === "top" || s === "bottom";

/** Right-angled route between two side anchors. */
function route(a: Pt, aSide: Side, b: Pt, bSide: Side, bend?: number): Pt[] {
  if (vertical(aSide) && vertical(bSide)) {
    if (a[0] === b[0]) return [a, b];
    const y = bend ?? (a[1] + b[1]) / 2;
    return [a, [a[0], y], [b[0], y], b];
  }
  if (!vertical(aSide) && !vertical(bSide)) {
    if (a[1] === b[1]) return [a, b];
    const x = bend ?? (a[0] + b[0]) / 2;
    return [a, [x, a[1]], [x, b[1]], b];
  }
  if (vertical(aSide)) return [a, [a[0], b[1]], b];
  return [a, [b[0], a[1]], b];
}

function unit(from: Pt, to: Pt): Pt {
  const dx = to[0] - from[0];
  const dy = to[1] - from[1];
  const len = Math.hypot(dx, dy) || 1;
  return [dx / len, dy / len];
}

/** Moves the last point back along its segment by `by`. */
function trimEnd(points: Pt[], by: number): Pt[] {
  const out = points.slice();
  const end = out[out.length - 1];
  const prev = out[out.length - 2];
  const [ux, uy] = unit(prev, end);
  out[out.length - 1] = [end[0] - ux * by, end[1] - uy * by];
  return out;
}

/** A polyline with rounded corners, and its length. */
function roundedPath(points: Pt[]): { d: string; length: number } {
  let d = `M${points[0][0]} ${points[0][1]}`;
  let length = 0;
  let cursor = points[0];
  for (let i = 1; i < points.length; i++) {
    const p = points[i];
    if (i === points.length - 1) {
      d += ` L${p[0]} ${p[1]}`;
      length += Math.hypot(p[0] - cursor[0], p[1] - cursor[1]);
      break;
    }
    const prev = points[i - 1];
    const next = points[i + 1];
    const r = Math.min(
      RADIUS,
      Math.hypot(p[0] - prev[0], p[1] - prev[1]) / 2,
      Math.hypot(next[0] - p[0], next[1] - p[1]) / 2,
    );
    const [ax, ay] = unit(prev, p);
    const [bx, by] = unit(p, next);
    const p1: Pt = [p[0] - ax * r, p[1] - ay * r];
    const p2: Pt = [p[0] + bx * r, p[1] + by * r];
    d += ` L${p1[0]} ${p1[1]} Q${p[0]} ${p[1]} ${p2[0]} ${p2[1]}`;
    length += Math.hypot(p1[0] - cursor[0], p1[1] - cursor[1]) + r * 1.6;
    cursor = p2;
  }
  return { d, length };
}

function arrowHead(tip: Pt, from: Pt): string {
  const [ux, uy] = unit(from, tip);
  const bx = tip[0] - ux * ARROW;
  const by = tip[1] - uy * ARROW;
  const px = -uy * 3.75;
  const py = ux * 3.75;
  const f = (n: number) => Math.round(n * 100) / 100;
  return `M${f(tip[0])} ${f(tip[1])} L${f(bx + px)} ${f(by + py)} L${f(bx - px)} ${f(by - py)} Z`;
}

const dash: Record<EdgeKind, string | undefined> = {
  flow: undefined,
  async: "5 4",
  dep: "0 4.5",
};

type DrawnEdge = {
  edge: DiagramEdge;
  points: Pt[];
  d: string;
  length: number;
  heads: string[];
};

function drawEdge(edge: DiagramEdge, boxes: Map<string, Box>): DrawnEdge {
  const a = parseEnd(edge.from, boxes);
  const b = parseEnd(edge.to, boxes);
  const points: Pt[] = edge.via ? [a.pt, ...edge.via, b.pt] : route(a.pt, a.side, b.pt, b.side, edge.bend);
  const arrow = edge.arrow ?? "end";
  let line = points;
  const heads: string[] = [];
  if (arrow === "end" || arrow === "both") {
    heads.push(arrowHead(points[points.length - 1], points[points.length - 2]));
    line = trimEnd(line, ARROW - 1);
  }
  if (arrow === "start" || arrow === "both") {
    heads.push(arrowHead(points[0], points[1]));
    line = trimEnd(line.slice().reverse(), ARROW - 1).reverse();
  }
  const { d, length } = roundedPath(line);
  return { edge, points, d, length, heads };
}

/** Times every step: a step starts when the longest line of the step before it has finished. */
function timing(drawn: DrawnEdge[]): { cycle: number; start: Map<number, number> } {
  const steps = [...new Set(drawn.flatMap((e) => (e.edge.step === undefined ? [] : [e.edge.step])))].sort((x, y) => x - y);
  const start = new Map<number, number>();
  let t = 0;
  for (const step of steps) {
    start.set(step, t);
    const longest = Math.max(...drawn.filter((e) => e.edge.step === step).map((e) => e.length));
    t += longest / SPEED + GAP;
  }
  return { cycle: t + REST, start };
}

function labelPosition(drawn: DrawnEdge): { at: Pt; anchor: "start" | "middle" | "end" } {
  // The longest segment, with the label beside it.
  let best = 0;
  let bestLen = -1;
  for (let i = 0; i < drawn.points.length - 1; i++) {
    const [x1, y1] = drawn.points[i];
    const [x2, y2] = drawn.points[i + 1];
    const len = Math.hypot(x2 - x1, y2 - y1);
    if (len > bestLen) {
      bestLen = len;
      best = i;
    }
  }
  const [x1, y1] = drawn.points[best];
  const [x2, y2] = drawn.points[best + 1];
  if (x1 === x2) return { at: [x1 + 8, (y1 + y2) / 2 + 4], anchor: "start" };
  return { at: [(x1 + x2) / 2, y1 - 8], anchor: "middle" };
}

const toneText = {
  "fg-1": "fill-fg-1",
  "fg-2": "fill-fg-2",
  "fg-3": "fill-fg-3",
  accent: "fill-accent",
} as const;

function Node({ n }: { n: DiagramNode }) {
  const h = nodeHeight(n);
  const accent = n.tone === "accent";
  const muted = n.tone === "muted";
  const rows = n.rows ?? [];
  const head = rows.length ? 56 : h;
  const center = (n.align ?? (n.icon ? "start" : "center")) === "center";
  const textX = center ? n.x + n.w / 2 : n.x + (n.icon ? 48 : 14);
  const anchor = center ? "middle" : "start";
  const twoLines = Boolean(n.sub);
  const titleY = twoLines ? n.y + 24 : n.y + head / 2 + 5;
  const chip = 28;
  const chipY = n.y + (head - chip) / 2;
  return (
    <g data-node={n.id}>
      <rect
        data-box={n.id}
        x={n.x + 0.5}
        y={n.y + 0.5}
        width={n.w - 1}
        height={h - 1}
        rx={10}
        className={cn(
          accent
            ? "fill-[rgb(61_255_143/0.07)] stroke-accent/70 [filter:drop-shadow(0_0_14px_rgb(61_255_143/0.22))]"
            : muted
              ? "fill-ink-1 stroke-line-2 [stroke-dasharray:3_3]"
              : "fill-ink-2 stroke-line-2",
        )}
      />
      {/* A faint top highlight, like light on a machined edge. */}
      {!muted ? (
        <path
          d={`M${n.x + 10} ${n.y + 1.5} H${n.x + n.w - 10}`}
          className={accent ? "stroke-accent/25" : "stroke-fg/[0.06]"}
          strokeWidth={1}
        />
      ) : null}
      {n.icon ? (
        <>
          <rect
            x={n.x + 12.5}
            y={chipY + 0.5}
            width={chip - 1}
            height={chip - 1}
            rx={7}
            className={accent ? "fill-accent/10 stroke-accent/40" : "fill-ink-3 stroke-line-2"}
          />
          <Glyph name={n.icon} x={n.x + 18} y={chipY + 6} className={accent ? "stroke-accent" : "stroke-fg-2"} />
        </>
      ) : null}
      <text
        data-in={n.id}
        x={textX}
        y={titleY}
        textAnchor={anchor}
        fontSize={n.mono ? 13 : 14}
        fontWeight={n.mono ? 400 : 500}
        className={cn(n.mono ? "font-mono" : "font-sans", muted ? "fill-fg-2" : "fill-fg")}
      >
        {n.title}
      </text>
      {n.sub ? (
        <text
          data-in={n.id}
          x={textX}
          y={n.y + 42}
          textAnchor={anchor}
          fontSize={12}
          className={cn(n.subMono ? "font-mono" : "font-sans", accent ? "fill-accent/85" : "fill-fg-3")}
        >
          {n.sub}
        </text>
      ) : null}
      {rows.map((row, i) => {
        const [name, note] = typeof row === "string" ? [row, undefined] : row;
        // Rows sit at the bottom of the node, 16 px from its edge.
        const y = n.y + h - 16 - (rows.length * 32 - 8) + i * 32;
        return (
          <g key={name + i}>
            <rect
              x={n.x + 12.5}
              y={y + 0.5}
              width={n.w - 25}
              height={23}
              rx={6}
              className={accent ? "fill-ink-1 stroke-accent/30" : "fill-ink-1 stroke-line-2"}
            />
            <text data-in={n.id} x={n.x + 24} y={y + 16} fontSize={12} className="fill-fg-1 font-mono">
              {name}
            </text>
            {note ? (
              <text
                data-in={n.id}
                x={n.x + n.w - 24}
                y={y + 16}
                textAnchor="end"
                fontSize={12}
                className="fill-fg-3 font-sans"
              >
                {note}
              </text>
            ) : null}
          </g>
        );
      })}
    </g>
  );
}

function Group({ g }: { g: DiagramGroup }) {
  const accent = g.tone === "accent";
  return (
    <g data-group={g.id ?? g.label}>
      <rect
        x={g.x + 0.5}
        y={g.y + 0.5}
        width={g.w - 1}
        height={g.h - 1}
        rx={14}
        strokeDasharray={g.dashed ? "4 4" : undefined}
        className={accent ? "fill-[rgb(61_255_143/0.025)] stroke-accent/55" : "fill-fg/[0.012] stroke-line-2"}
      />
      <text
        data-label
        x={g.x + 16}
        y={g.y + 22}
        fontSize={12}
        letterSpacing="0.06em"
        className={cn("font-mono uppercase", accent ? "fill-accent" : "fill-fg-3")}
      >
        {g.label}
      </text>
    </g>
  );
}

function EdgeLabel({ drawn }: { drawn: DrawnEdge }) {
  const { edge } = drawn;
  if (!edge.label) return null;
  const auto = labelPosition(drawn);
  const at = edge.at ?? auto.at;
  const anchor = edge.anchor ?? (edge.at ? "start" : auto.anchor);
  const lines = edge.label.split("\n");
  return (
    <text
      data-label
      x={at[0]}
      y={at[1]}
      textAnchor={anchor}
      fontSize={12}
      className={cn(edge.labelMono === false ? "font-sans" : "font-mono", "fill-fg-3")}
    >
      {lines.map((line, i) => (
        <tspan key={line} x={at[0]} dy={i === 0 ? 0 : 16}>
          {line}
        </tspan>
      ))}
    </text>
  );
}

/**
 * One layout drawn as an SVG. `title` and `desc` give it its accessible
 * name and description. `maxScale` caps how far it grows past 1:1.
 */
export function DiagramSvg({
  layout,
  title,
  desc,
  maxScale = 1.2,
  className,
}: {
  layout: DiagramLayout;
  title: string;
  desc: string;
  maxScale?: number;
  className?: string;
}) {
  const boxes = new Map<string, Box>();
  for (const g of layout.groups ?? []) if (g.id) boxes.set(g.id, g);
  for (const n of layout.nodes) boxes.set(n.id, { x: n.x, y: n.y, w: n.w, h: nodeHeight(n) });
  const drawn = layout.edges.map((e) => drawEdge(e, boxes));
  const { cycle, start } = timing(drawn);
  const width = layout.width + MARGIN * 2;
  const height = layout.height + MARGIN * 2;

  return (
    <svg
      role="img"
      viewBox={`${-MARGIN} ${-MARGIN} ${width} ${height}`}
      width={width}
      height={height}
      style={{ maxWidth: `min(100%, ${Math.round(width * maxScale)}px)` }}
      className={cn("mx-auto block h-auto w-full", className)}
      data-diagram-svg=""
      data-width={layout.width}
    >
      <title>{title}</title>
      <desc>{desc}</desc>
      {(layout.groups ?? []).map((g) => (
        <Group key={g.id ?? g.label} g={g} />
      ))}
      <g data-edges="">
        {drawn.map((e, i) => (
          <g key={i} data-edge={`${e.edge.from}>${e.edge.to}`} data-points={e.points.map((p) => p.join(",")).join(" ")}>
            <path
              d={e.d}
              fill="none"
              strokeWidth={e.edge.kind === "dep" ? 1.75 : 1.25}
              strokeDasharray={dash[e.edge.kind ?? "flow"]}
              strokeLinecap="round"
              className="stroke-bar"
            />
            {e.heads.map((h) => (
              <path key={h} d={h} className="fill-bar" />
            ))}
          </g>
        ))}
      </g>
      {layout.extras}
      {layout.nodes.map((n) => (
        <Node key={n.id} n={n} />
      ))}
      {drawn.map((e, i) => (
        <EdgeLabel key={i} drawn={e} />
      ))}
      {(layout.notes ?? []).map((note) => (
        <text
          key={`${note.x},${note.y}`}
          data-label
          x={note.x}
          y={note.y}
          textAnchor={note.anchor ?? "start"}
          fontSize={note.size ?? 12}
          className={cn(note.mono ? "font-mono" : "font-sans", toneText[note.tone ?? "fg-3"])}
        >
          {note.text}
        </text>
      ))}
      <g aria-hidden="true" data-pulses="">
        {drawn.map((e, i) => {
          if (e.edge.step === undefined) return null;
          const delay = start.get(e.edge.step) ?? 0;
          const travel = Math.ceil(cycle * SPEED + 80);
          const vars = (len: number) =>
            ({
              "--dash": `${len}px`,
              "--travel": `${travel}px`,
              "--cycle": `${cycle.toFixed(2)}s`,
              "--delay": `${delay.toFixed(2)}s`,
            }) as CSSProperties;
          return (
            <g key={i}>
              <path d={e.d} className="diagram-pulse stroke-accent/25" strokeWidth={5} style={vars(34)} />
              <path d={e.d} className="diagram-pulse stroke-accent" strokeWidth={1.75} style={vars(12)} />
            </g>
          );
        })}
      </g>
    </svg>
  );
}
