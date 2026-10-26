/**
 * A naval architect's "body plan": the hull's cross-sections (stations)
 * seen from ahead, aft half on the left and forward half on the right,
 * over the waterlines. The design waterline (DWL) and the fin are in the
 * accent. Generated here, at build time, from a small parametric hull.
 */

const W = 640;
const H = 470;
const CX = W / 2;
const BL = 392; // baseline (y)
const DWL = 262; // design waterline (y)
const B = 226; // half-breadth midships
const ROOT = 4; // half-width of the bottom at the centreline

type Station = { s: number; side: -1 | 1 };

/** Station numbers 0 (stern) to 10 (bow). Aft ones are drawn on the left. */
const stations: Station[] = [
  ...[0, 1, 2, 3, 4, 5].map((s) => ({ s, side: -1 as const })),
  ...[5, 6, 7, 8, 8.75].map((s) => ({ s, side: 1 as const })),
];

/**
 * One station: from the centreline, a bottom with deadrise, a round bilge, then
 * the topsides up to the deck (flaring out towards the bow).
 */
function sectionPath({ s, side }: Station): string {
  const t = (s - 5) / 5; // -1 stern … 0 midships … 1 bow
  const a = Math.abs(t);
  const forward = t > 0;
  // Half-breadth at the bilge: fine at the bow, a wide transom at the stern.
  const bw = forward ? B * Math.max(0.03, 1 - a ** 1.9) : B * (1 - 0.48 * a ** 2.2);
  // Half-breadth at the deck: the bow flares out.
  const bd = Math.min(B * 1.02, forward ? bw + 34 * a ** 1.3 * (1 - a ** 4) + 6 * a : bw * (1 - 0.02 * a));
  // Deck (sheer) height: rises towards the bow.
  const yDeck = 120 - (forward ? 34 * a ** 2 : 12 * a ** 2);
  // Bottom at the centreline: the forefoot and the run rise.
  const yBottom = forward
    ? BL - 168 * Math.max(0, (a - 0.45) / 0.55) ** 1.6
    : BL - 112 * Math.max(0, (a - 0.5) / 0.5) ** 1.45;
  // Deadrise (how much the bottom climbs to the bilge) and bilge radius.
  const rise = forward ? 12 + 34 * a ** 0.9 : 12 + 6 * a;
  const depth = yBottom - yDeck;
  const radius = Math.min(forward ? 72 + depth * 0.55 * a ** 1.2 : 72 + 46 * a ** 2, (bw - ROOT) * 0.82, (depth - rise) * 0.8);
  const bilgeX = Math.max(ROOT, bw - radius);
  const bilgeY = yBottom - rise;
  const turnY = bilgeY - radius;
  // Continue the bottom's direction into the bilge, so there is no kink.
  const dx = bilgeX - ROOT;
  const dy = bilgeY - yBottom;
  const len = Math.hypot(dx, dy) || 1;
  const pull = radius * 0.56;
  const x = (v: number) => (CX + side * v).toFixed(1);
  const y = (v: number) => v.toFixed(1);
  return [
    `M${x(ROOT)} ${y(yBottom)}`,
    `L${x(bilgeX)} ${y(bilgeY)}`,
    `C${x(bilgeX + (dx / len) * pull)} ${y(bilgeY + (dy / len) * pull)} ${x(bw)} ${y(bilgeY - radius * 0.44)} ${x(bw)} ${y(turnY)}`,
    `C${x(bw)} ${y(turnY - (turnY - yDeck) * 0.45)} ${x(bd - (bd - bw) * 0.2)} ${y(yDeck + (turnY - yDeck) * 0.25)} ${x(bd)} ${y(yDeck)}`,
  ].join(" ");
}

const waterlines = [182, 222, DWL, 302, 342];
const buttocks = [78, 156];

/** The drawing's geometry, for other renderers (the OG image). */
export const hull = {
  width: W,
  height: H,
  centre: CX,
  baseline: BL,
  waterline: DWL,
  fin: `M${CX - 9} ${BL}H${CX + 9}L${CX + 6} ${BL + 22}H${CX - 6}Z`,
  stations: stations.map((station) => ({ d: sectionPath(station), mid: station.s === 5 })),
  waterlines: waterlines.filter((y) => y !== DWL),
  buttocks,
};

export function HullDrawing({ className }: { className?: string }) {
  return (
    <svg
      viewBox={`0 0 ${W} ${H}`}
      className={`hull ${className ?? ""}`}
      role="img"
      aria-label="A body plan drawing of a ship's hull: its cross-sections over the waterlines, the design waterline and the fin below the hull highlighted in green."
    >
      <defs>
        {/* User-space units: a horizontal line has a zero-height bounding box. */}
        <filter id="hull-glow" filterUnits="userSpaceOnUse" x={0} y={0} width={W} height={H}>
          <feGaussianBlur stdDeviation="3.5" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>

      {/* Reference lines: waterlines, buttocks, centreline, baseline. */}
      <g className="hull-grid">
        {waterlines
          .filter((y) => y !== DWL)
          .map((y) => (
            <line key={y} x1={48} x2={W - 48} y1={y} y2={y} />
          ))}
        {buttocks.flatMap((d) => [
          <line key={`l${d}`} x1={CX - d} x2={CX - d} y1={96} y2={BL} />,
          <line key={`r${d}`} x1={CX + d} x2={CX + d} y1={96} y2={BL} />,
        ])}
        <line className="hull-ref" x1={CX} x2={CX} y1={44} y2={BL + 26} />
        <line className="hull-ref" x1={40} x2={W - 40} y1={BL} y2={BL} />
      </g>

      {/* Stations. */}
      <g className="hull-stations">
        {stations.map((station, i) => (
          <path
            key={`${station.s}-${station.side}`}
            d={sectionPath(station)}
            pathLength={1}
            className={station.s === 5 ? "hull-station is-mid" : "hull-station"}
            style={{ animationDelay: `${120 + i * 55}ms` }}
          />
        ))}
      </g>

      {/* Design waterline and fin. */}
      <line className="hull-dwl" x1={36} x2={W - 36} y1={DWL} y2={DWL} filter="url(#hull-glow)" />
      <path className="hull-fin" d={`M${CX - 9} ${BL}H${CX + 9}L${CX + 6} ${BL + 22}H${CX - 6}Z`} filter="url(#hull-glow)" />

      {/* Drafting labels. */}
      <g className="hull-labels">
        <text x={W - 36} y={DWL - 9} textAnchor="end">
          DWL
        </text>
        <text x={W - 40} y={BL - 9} textAnchor="end">
          BL
        </text>
        <text x={CX + 8} y={58}>
          CL
        </text>
        <text x={48} y={58}>
          Aft
        </text>
        <text x={W - 48} y={58} textAnchor="end">
          Fore
        </text>
      </g>
    </svg>
  );
}
