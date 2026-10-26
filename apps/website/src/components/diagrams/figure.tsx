import { cn } from "@/lib/utils";

import { DiagramSvg, type DiagramLayout, type EdgeKind } from "./kit";

/** A diagram: its words, its legend and its two layouts. */
export type DiagramSpec = {
  /** The accessible name, also shown in the caption. */
  title: string;
  /** What the picture shows, in words, for screen readers. */
  desc: string;
  /** One legend entry per kind of line the diagram uses. */
  legend: { kind: EdgeKind; label: string }[];
  /** At least 592 px wide: shown when it fits at 1:1. */
  wide: DiagramLayout;
  /** A vertical re-layout for phones and narrow columns, 272 px wide. */
  narrow: DiagramLayout;
};

/*
 * The wide layout (592 px plus a 16 px margin each side) is shown only when
 * its container fits it at 1:1, so its 12 px labels never render smaller.
 * Below that the narrow layout takes over. The switch is a container query
 * at 620 px (the `@min-[38.75rem]` classes below), so the same diagram picks
 * the right layout in a docs column (622 px at 1280), a page section or a
 * phone.
 */

function LegendLine({ kind }: { kind: EdgeKind }) {
  return (
    <svg width="28" height="8" viewBox="0 0 28 8" aria-hidden="true" className="shrink-0">
      <path
        d="M1 4H20"
        fill="none"
        strokeWidth={kind === "dep" ? 1.75 : 1.25}
        strokeLinecap="round"
        strokeDasharray={kind === "async" ? "5 4" : kind === "dep" ? "0 4.5" : undefined}
        className="stroke-fg-3"
      />
      <path d="M27 4 L20 7.75 L20 0.25 Z" className="fill-fg-3" />
    </svg>
  );
}

export function DiagramLegend({ legend, className }: { legend: DiagramSpec["legend"]; className?: string }) {
  if (legend.length === 0) return null;
  return (
    <ul className={cn("m-0 flex list-none flex-wrap items-center gap-x-5 gap-y-1.5 p-0", className)} aria-label="Legend">
      {legend.map((item) => (
        <li key={item.kind} className="m-0 flex items-center gap-2 p-0 text-xs text-fg-3">
          <LegendLine kind={item.kind} />
          {item.label}
        </li>
      ))}
    </ul>
  );
}

/** Both layouts, switched by the width of their container. */
export function DiagramCanvas({
  spec,
  className,
  maxScale = 1.2,
}: {
  spec: DiagramSpec;
  className?: string;
  /** How far a layout may grow past 1:1 in a wide container. */
  maxScale?: number;
}) {
  return (
    <div className={cn("@container", className)}>
      <div className="hidden @min-[38.75rem]:block">
        <DiagramSvg layout={spec.wide} title={spec.title} desc={spec.desc} maxScale={maxScale} />
      </div>
      {/*
        Below 304 px (a 320 px phone) the narrow layout borrows 10 px of its
        own empty margin on each side, so it still draws at 1:1 and no text
        drops under 12 px.
      */}
      <div className="@min-[38.75rem]:hidden @max-[19rem]:-mx-2.5">
        <DiagramSvg layout={spec.narrow} title={spec.title} desc={spec.desc} maxScale={1.2} />
      </div>
    </div>
  );
}

/**
 * The framed diagram used in the docs and on the site: a dotted canvas,
 * the drawing, and a caption bar with the title and the legend.
 */
export function DiagramFigure({
  spec,
  name,
  className,
  caption = true,
  fill = false,
  maxScale,
}: {
  spec: DiagramSpec;
  name: string;
  className?: string;
  caption?: boolean;
  /** Grow to the height of the parent and centre the drawing in it. */
  fill?: boolean;
  maxScale?: number;
}) {
  return (
    <figure
      data-diagram={name}
      className={cn(
        "diagram-figure overflow-hidden rounded-2xl border border-line bg-ink-1 shadow-bezel",
        fill && "flex h-full flex-col",
        className,
      )}
    >
      <div className={cn("relative", fill && "flex flex-1 flex-col justify-center")}>
        <div
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle,var(--color-line-2)_1px,transparent_1.25px)] [mask-image:radial-gradient(ellipse_at_center,black_35%,transparent_85%)] bg-size-[16px_16px] opacity-60"
        />
        <DiagramCanvas spec={spec} className="relative py-3" maxScale={maxScale} />
      </div>
      {caption ? (
        <figcaption className="flex flex-wrap items-center justify-between gap-x-6 gap-y-2 border-t border-line px-4 py-3">
          <span className="text-[0.8125rem] font-medium text-fg-2">{spec.title}</span>
          <DiagramLegend legend={spec.legend} />
        </figcaption>
      ) : null}
    </figure>
  );
}
