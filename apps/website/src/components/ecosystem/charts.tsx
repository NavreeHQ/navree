import { cn } from "@/lib/utils";

/**
 * A sparkline: a line (and optional faint area) through `data`, stretched to
 * its box. Colour comes from `currentColor`.
 */
export function Sparkline({
  data,
  className,
  area = false,
  strokeWidth = 1.5,
}: {
  data: number[];
  className?: string;
  area?: boolean;
  strokeWidth?: number;
}) {
  const w = 100;
  const h = 32;
  const max = Math.max(...data);
  const min = Math.min(...data);
  const span = max - min || 1;
  const step = w / Math.max(1, data.length - 1);
  const points = data.map((v, i) => [i * step, h - 2 - ((v - min) / span) * (h - 4)] as const);
  const line = points.map(([x, y], i) => `${i ? "L" : "M"}${x.toFixed(2)} ${y.toFixed(2)}`).join(" ");
  return (
    <svg viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" className={cn("block", className)} aria-hidden>
      {area ? <path d={`${line} L${w} ${h} L0 ${h} Z`} fill="currentColor" opacity={0.12} /> : null}
      <path d={line} fill="none" stroke="currentColor" strokeWidth={strokeWidth} vectorEffect="non-scaling-stroke" strokeLinejoin="round" strokeLinecap="round" />
    </svg>
  );
}

/** Vertical bars over time. `accentFrom` colours the bars from that index on. */
export function Bars({
  data,
  className,
  barClassName = "fill-line-2",
  accentFrom,
  accentClassName = "fill-accent/70",
  gap = 0.28,
}: {
  data: number[];
  className?: string;
  barClassName?: string;
  accentFrom?: number;
  accentClassName?: string;
  gap?: number;
}) {
  const w = 100;
  const h = 40;
  const max = Math.max(...data) || 1;
  const bw = w / data.length;
  return (
    <svg viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" className={cn("block", className)} aria-hidden>
      {data.map((v, i) => {
        const bh = Math.max(0.6, (v / max) * (h - 1));
        return (
          <rect
            key={i}
            x={i * bw + (bw * gap) / 2}
            y={h - bh}
            width={bw * (1 - gap)}
            height={bh}
            rx={0.4}
            className={accentFrom !== undefined && i >= accentFrom ? accentClassName : barClassName}
          />
        );
      })}
    </svg>
  );
}
