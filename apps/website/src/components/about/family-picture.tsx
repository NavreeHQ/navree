import { picture } from "@/content/about";
import { cn } from "@/lib/utils";

import { FamilyLive } from "./family-live";
import { CODE, CODE_LAYOUT, CODE_TITLE, HEAD_OVER_SCREEN, IMAGE, SCREEN, WIDTHS, codeBaseline, codeX, imageSrc } from "./picture";

/** On phones the frame is wider than the screen and cropped to the family, so they are drawn larger. */
const FRAME = "w-full max-sm:-ml-[18%] max-sm:w-[135%]";
/** How wide the picture is drawn: 7 of 12 columns from 1280 px, the container's width below, 135% of a phone's. */
const SIZES = "(min-width: 1280px) min(58vw, 820px), (min-width: 640px) calc(100vw - 48px), 135vw";

const srcSet = (ext: "avif" | "webp") => WIDTHS.map((w) => `${imageSrc(w, ext)} ${w}w`).join(", ");

/** The tone of a line of code: attributes brightest, the body of a function a step down. */
function codeTone(line: string): { fill: string; opacity: number } {
  if (line.trimStart().startsWith("#[")) return { fill: "#94ffbf", opacity: 0.95 };
  if (line.startsWith("        ")) return { fill: "#5cffa1", opacity: 0.72 };
  return { fill: "#5cffa1", opacity: 0.85 };
}

/**
 * Real Navree code over the picture's screen, whose own text is made-up
 * glyphs: the glass is dimmed and the code set on it, in the picture's own
 * coordinates, so it tracks the picture at every width. Navreet's head,
 * which stands in front of the screen, stays clear.
 */
function ScreenCode() {
  const glass = { x: SCREEN.x, y: SCREEN.y, width: SCREEN.width, height: SCREEN.height };
  return (
    <svg
      viewBox={`0 0 ${IMAGE.width} ${IMAGE.height}`}
      aria-hidden
      className="pointer-events-none absolute inset-0 size-full font-mono select-none"
    >
      <defs>
        <mask id="about-glass" maskUnits="userSpaceOnUse" x="0" y="0" width={IMAGE.width} height={IMAGE.height}>
          <rect {...glass} fill="#fff" />
          <polygon points={HEAD_OVER_SCREEN.map((p) => p.join(",")).join(" ")} fill="#000" />
        </mask>
      </defs>
      <g mask="url(#about-glass)">
        <rect {...glass} fill="#030d08" fillOpacity={0.88} />
        <g fontSize={CODE_LAYOUT.size}>
          <text x={codeX(0)} y={CODE_LAYOUT.title} fill="#3dff8f" fillOpacity={0.5}>
            {CODE_TITLE}
          </text>
          {CODE.map((line, i) => {
            const indent = line.length - line.trimStart().length;
            const tone = codeTone(line);
            return (
              <g key={i}>
                <text x={CODE_LAYOUT.gutter} y={codeBaseline(i)} textAnchor="end" fill="#3dff8f" fillOpacity={0.32}>
                  {i + 1}
                </text>
                {line.trim() ? (
                  <text x={codeX(indent)} y={codeBaseline(i)} fill={tone.fill} fillOpacity={tone.opacity}>
                    {line.trim()}
                  </text>
                ) : null}
              </g>
            );
          })}
        </g>
      </g>
    </svg>
  );
}

/**
 * The about picture: Amritpal at his desk with Navraj on his lap and Navreet
 * beside him, seen from behind, drawn in glowing lines and ones and zeros,
 * with real Navree code on the screen. This still is the whole picture
 * without JavaScript, under reduced motion and on devices that can't run
 * the live one; `FamilyLive` brings the same picture to life over it where
 * they can, and the still fades out (it stays in the page for screen
 * readers).
 */
export function FamilyPicture({ className }: { className?: string }) {
  return (
    <figure className={cn("min-w-0", className)}>
      <div className="group/picture relative max-sm:-mx-4 max-sm:overflow-hidden">
        <div className={cn("relative transition-opacity duration-700 ease-(--ease-out-quint) group-data-live/picture:opacity-0", FRAME)}>
          <picture>
            <source type="image/avif" srcSet={srcSet("avif")} sizes={SIZES} />
            <source type="image/webp" srcSet={srcSet("webp")} sizes={SIZES} />
            {/* oxlint-disable-next-line next/no-img-element -- static export: srcset by hand */}
            <img
              src={imageSrc(1280, "webp")}
              width={IMAGE.width}
              height={IMAGE.height}
              alt={picture.alt}
              fetchPriority="high"
              decoding="async"
              data-about-picture=""
              className="block h-auto w-full"
            />
          </picture>
          <ScreenCode />
        </div>
        <FamilyLive frameClass={FRAME} />
      </div>
      <figcaption className="mt-3 text-sm leading-relaxed text-pretty text-fg-3 max-sm:mt-1">{picture.caption}</figcaption>
    </figure>
  );
}
