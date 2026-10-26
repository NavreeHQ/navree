import fs from "node:fs";
import path from "node:path";

import { ImageResponse } from "next/og";

import { MARK_PATHS } from "@/components/site/logo";
import { site } from "@site";

const size = { width: 1200, height: 630 };

export const dynamic = "force-static";

const font = (file: string) => fs.readFileSync(path.join(process.cwd(), "node_modules", "@fontsource", file));

const INK0 = "#040605";
const INK1 = "#0a0e0c";
const INK2 = "#111714";
const LINE = "#1c2420";
const LINE2 = "#2a3530";
const FG = "#e9efeb";
const FG2 = "#a9b5ae";
const FG3 = "#8a968f";
const ACCENT = "#3dff8f";

/** The module graph on the right: the app module (green) wired to its feature modules. */
const HUB = { x: 944, y: 300 };
const PILL = { w: 112, h: 44 };
const MODULES = [
  { label: "auth", x: 824, y: 168 },
  { label: "workspace", x: 1064, y: 168 },
  { label: "billing", x: 772, y: 300 },
  { label: "queue", x: 1116, y: 300 },
  { label: "search", x: 824, y: 432 },
  { label: "ai", x: 1064, y: 432 },
];

/**
 * `/og.png`: the social card (the mark, the category line, the tagline and
 * a module graph over a hairline grid, cut by the green waterline), written
 * to `out/og.png` at build time. A route handler rather than
 * `opengraph-image.tsx` so the exported file keeps its .png extension and
 * static hosts serve it as an image.
 */
export function GET() {
  const gridX = Array.from({ length: 19 }, (_, i) => i * 64 + 24);
  const gridY = Array.from({ length: 10 }, (_, i) => i * 64 + 23);
  return new ImageResponse(
    (
      <div
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          position: "relative",
          background: INK0,
          color: FG,
          fontFamily: "Geist",
        }}
      >
        {/* Hairline grid, brightest behind the graph. */}
        <svg width="1200" height="630" viewBox="0 0 1200 630" style={{ position: "absolute", left: 0, top: 0 }}>
          <defs>
            <radialGradient id="fade" cx="0.8" cy="0.48" r="0.62">
              <stop offset="0" stopColor="#ffffff" stopOpacity="1" />
              <stop offset="1" stopColor="#ffffff" stopOpacity="0.18" />
            </radialGradient>
            <mask id="m">
              <rect width="1200" height="630" fill="url(#fade)" />
            </mask>
            <radialGradient id="glow" cx="0.5" cy="0.5" r="0.5">
              <stop offset="0" stopColor={ACCENT} stopOpacity="0.22" />
              <stop offset="1" stopColor={ACCENT} stopOpacity="0" />
            </radialGradient>
          </defs>
          <g mask="url(#m)">
            {gridX.map((x) => (
              <line key={`x${x}`} x1={x} x2={x} y1={0} y2={630} stroke={LINE} strokeWidth={1} />
            ))}
            {gridY.map((y) => (
              <line key={`y${y}`} x1={0} x2={1200} y1={y} y2={y} stroke={LINE} strokeWidth={1} />
            ))}
          </g>
          <circle cx={HUB.x} cy={HUB.y} r={230} fill="url(#glow)" />
          {/* Wires from the hub to each module. */}
          {MODULES.map((m) => (
            <line key={m.label} x1={HUB.x} y1={HUB.y} x2={m.x} y2={m.y} stroke={LINE2} strokeWidth={1.5} />
          ))}
          {MODULES.map((m) => (
            <g key={`n${m.label}`}>
              <rect x={m.x - PILL.w / 2} y={m.y - PILL.h / 2} width={PILL.w} height={PILL.h} rx={PILL.h / 2} fill={INK1} stroke={LINE2} strokeWidth={1.5} />
            </g>
          ))}
          <rect x={HUB.x - 84} y={HUB.y - 30} width={168} height={60} rx={30} fill={INK2} stroke={ACCENT} strokeWidth={2} />
          {/* The waterline. */}
          <rect x={0} y={566} width={1200} height={2} fill={ACCENT} />
          <rect x={0} y={560} width={1200} height={14} fill={ACCENT} opacity={0.08} />
        </svg>

        {/* Module labels (text in HTML so it uses the loaded fonts). */}
        {MODULES.map((m) => (
          <div
            key={`t${m.label}`}
            style={{
              position: "absolute",
              left: m.x - PILL.w / 2,
              top: m.y - PILL.h / 2,
              width: PILL.w,
              height: PILL.h,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              fontSize: 19,
              color: FG2,
            }}
          >
            {m.label}
          </div>
        ))}
        <div
          style={{
            position: "absolute",
            left: HUB.x - 84,
            top: HUB.y - 30,
            width: 168,
            height: 60,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            fontSize: 21,
            fontWeight: 600,
            color: ACCENT,
          }}
        >
          AppModule
        </div>

        {/* Copy. */}
        <div
          style={{
            position: "absolute",
            left: 72,
            top: 64,
            bottom: 96,
            width: 640,
            display: "flex",
            flexDirection: "column",
            justifyContent: "space-between",
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
            <svg width="56" height="56" viewBox="0 0 32 32">
              {MARK_PATHS.map((d) => (
                <path key={d} d={d} fill={ACCENT} />
              ))}
            </svg>
            <span style={{ fontFamily: "Mona Sans", fontSize: 44, fontWeight: 700, letterSpacing: "-0.03em" }}>navree</span>
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 22 }}>
            <div
              style={{
                display: "flex",
                fontFamily: "Mona Sans",
                fontSize: 66,
                fontWeight: 700,
                lineHeight: 1.02,
                letterSpacing: "-0.035em",
              }}
            >
              {site.headline}
            </div>
            <div style={{ display: "flex", fontSize: 27, lineHeight: 1.35, color: FG2, maxWidth: 600 }}>{site.tagline}</div>
          </div>
        </div>
        <div
          style={{
            position: "absolute",
            left: 72,
            right: 72,
            bottom: 18,
            display: "flex",
            justifyContent: "space-between",
            fontSize: 20,
            color: FG3,
          }}
        >
          <span>{site.domain}</span>
          <span>Open source, {site.license}</span>
        </div>
      </div>
    ),
    {
      ...size,
      fonts: [
        { name: "Mona Sans", data: font("mona-sans/files/mona-sans-latin-700-normal.woff"), weight: 700, style: "normal" },
        { name: "Geist", data: font("geist/files/geist-latin-400-normal.woff"), weight: 400, style: "normal" },
        { name: "Geist", data: font("geist/files/geist-latin-600-normal.woff"), weight: 600, style: "normal" },
      ],
    },
  );
}
