import { ImageResponse } from "next/og";

import { hull } from "@/components/home/hull-drawing";
import { MARK_PATHS } from "@/components/site/logo";
import { site } from "@site";

const size = { width: 1200, height: 630 };

export const dynamic = "force-static";

/**
 * `/og.png`: the social card (the mark, the promise and the body plan
 * drawing), written to `out/og.png` at build time. A route handler rather
 * than `opengraph-image.tsx` so the exported file keeps its .png extension
 * and static hosts serve it as an image.
 */
export function GET() {
  const { width, height, centre, baseline, waterline } = hull;
  return new ImageResponse(
    (
      <div
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          background: "#040605",
          color: "#e9efeb",
          padding: "72px 80px",
          position: "relative",
        }}
      >
        <div style={{ display: "flex", flexDirection: "column", justifyContent: "space-between", width: 660 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 18 }}>
            <svg width="64" height="64" viewBox="0 0 32 32">
              {MARK_PATHS.map((d) => (
                <path key={d} d={d} fill="#3dff8f" />
              ))}
            </svg>
            <span style={{ fontSize: 52, fontWeight: 600, letterSpacing: "-0.03em" }}>navree</span>
          </div>
          <div style={{ display: "flex", flexDirection: "column", fontSize: 54, fontWeight: 600, lineHeight: 1.05, letterSpacing: "-0.035em" }}>
            {site.tagline.split(/(?<=,)\s+/).map((line) => (
              <span key={line}>{line}</span>
            ))}
          </div>
          <span style={{ fontSize: 26, color: "#8a968f" }}>{site.domain}</span>
        </div>
        <svg
          width={width * 0.78}
          height={height * 0.78}
          viewBox={`0 0 ${width} ${height}`}
          style={{ position: "absolute", right: 40, top: 130 }}
        >
          {hull.waterlines.map((y) => (
            <line key={y} x1={48} x2={width - 48} y1={y} y2={y} stroke="#1c2420" strokeWidth={1.4} />
          ))}
          <line x1={centre} x2={centre} y1={44} y2={baseline + 26} stroke="#2a3530" strokeWidth={1.4} />
          <line x1={40} x2={width - 40} y1={baseline} y2={baseline} stroke="#2a3530" strokeWidth={1.4} />
          {hull.stations.map((s) => (
            <path key={s.d} d={s.d} fill="none" stroke={s.mid ? "#a9b5ae" : "#8a968f"} strokeWidth={s.mid ? 2.2 : 1.6} />
          ))}
          <line x1={36} x2={width - 36} y1={waterline} y2={waterline} stroke="#3dff8f" strokeWidth={2.4} />
          <path d={hull.fin} fill="#3dff8f" />
        </svg>
      </div>
    ),
    size,
  );
}
