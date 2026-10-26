import { useId, type SVGProps } from "react";

import { cn } from "@/lib/utils";

interface GridPatternProps extends Omit<SVGProps<SVGSVGElement>, "width" | "height" | "x" | "y"> {
  /** Cell width, px. Keep it a multiple of 8. */
  width?: number;
  /** Cell height, px. */
  height?: number;
  x?: number;
  y?: number;
  /** Cells to fill (column, row), e.g. to light a few squares green. */
  squares?: Array<[x: number, y: number]>;
  strokeDasharray?: string;
  className?: string;
}

/**
 * A hairline grid background (static, server-rendered). Lines use `line`,
 * filled squares a faint green. Absolutely positioned; fade it with a mask.
 */
export function GridPattern({
  width = 40,
  height = 40,
  x = -1,
  y = -1,
  strokeDasharray = "0",
  squares,
  className,
  ...props
}: GridPatternProps) {
  const id = useId();

  return (
    <svg
      aria-hidden="true"
      className={cn("pointer-events-none absolute inset-0 size-full fill-accent/[0.06] stroke-line", className)}
      {...props}
    >
      <defs>
        <pattern id={id} width={width} height={height} patternUnits="userSpaceOnUse" x={x} y={y}>
          <path d={`M.5 ${height}V.5H${width}`} fill="none" strokeDasharray={strokeDasharray} />
        </pattern>
      </defs>
      <rect width="100%" height="100%" strokeWidth={0} fill={`url(#${id})`} />
      {squares ? (
        <svg x={x} y={y} className="overflow-visible">
          {squares.map(([sx, sy]) => (
            <rect strokeWidth="0" key={`${sx}-${sy}`} width={width - 1} height={height - 1} x={sx * width + 1} y={sy * height + 1} />
          ))}
        </svg>
      ) : null}
    </svg>
  );
}
