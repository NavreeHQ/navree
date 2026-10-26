import { ImageResponse } from "next/og";

import { MARK_PATHS } from "@/components/site/logo";

const size = { width: 180, height: 180 };

export const dynamic = "force-static";

/** `/apple-touch-icon.png`: the home-screen icon, the mark on near-black, built at build time. */
export function GET() {
  return new ImageResponse(
    (
      <div style={{ width: "100%", height: "100%", display: "flex", alignItems: "center", justifyContent: "center", background: "#040605" }}>
        <svg width="132" height="132" viewBox="0 0 32 32">
          {MARK_PATHS.map((d) => (
            <path key={d} d={d} fill="#3dff8f" />
          ))}
        </svg>
      </div>
    ),
    size,
  );
}
