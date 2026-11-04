import { LIGHT, lightStops } from "./picture-light.ts";
import { backgroundRain, backrest, cloth, furniture, insideFigures, layers, monitor, screenCode, screenMark } from "./picture-model.ts";
import { HEAD, LEVELS, VIEW, cellBaseline, cellX, cells, firstDrops, grids, levels, random, type Grid } from "./picture-rain.ts";

/**
 * The picture as a tree of SVG elements, `{ t: tag, a: attributes, c:
 * children }`, with SVG's own attribute names. `family-picture.tsx` turns it
 * into React elements; nothing here depends on React, so the same tree can
 * also be written out as markup.
 *
 * Every id starts with `ab-`. The animation reads the figure layers back
 * (`#ab-l-<id>`, each with `data-halo`) to cut its digits to the same
 * outline.
 */
export type El = { t: string; a?: Record<string, string | number | undefined>; c?: (El | string)[] };

export const GREEN = "#3dff8f";
export const MINT = "#d9ffe8";
const INK = "#040605";

const el = (t: string, a?: El["a"], c?: El["c"]): El => ({ t, a, c });
const box = { x: VIEW.x, y: VIEW.y, width: VIEW.width, height: VIEW.height };
const rect = (a: El["a"]) => el("rect", { ...box, ...a });
const r1 = (n: number) => Math.round(n * 10) / 10;
const r3 = (n: number) => Math.round(n * 1000) / 1000;

/** The light as a radial gradient in user space, squashed vertically as `lightAt` is. */
function lightGradient(id: string, color: string, gain: number): El {
  const t = `translate(${LIGHT.x} ${LIGHT.y}) scale(1 ${r3(1 / LIGHT.squash)}) translate(${-LIGHT.x} ${-LIGHT.y})`;
  return el(
    "radialGradient",
    { id, gradientUnits: "userSpaceOnUse", cx: LIGHT.x, cy: LIGHT.y, r: LIGHT.reach, gradientTransform: t },
    lightStops().map((s) => el("stop", { offset: r3(s.offset), "stop-color": color, "stop-opacity": r3(Math.min(1, s.value * gain)) })),
  );
}

/** A vertical fade from transparent (top) to `opacity` (bottom). */
function fade(id: string, color: string, opacity: number): El {
  return el("linearGradient", { id, x1: 0, y1: 0, x2: 0, y2: 1 }, [
    el("stop", { offset: 0, "stop-color": color, "stop-opacity": 0 }),
    el("stop", { offset: 1, "stop-color": color, "stop-opacity": opacity }),
  ]);
}

/** The cells of a grid that the figures reach: a corner or the centre inside a figure. */
export function coveredCells(g: Grid): boolean[] {
  const out: boolean[] = [];
  for (let row = 0; row < g.rows; row++) {
    for (let col = 0; col < g.cols; col++) {
      const x = g.x + col * g.cw;
      const y = g.y + row * g.ch;
      out.push(
        [
          [x + g.cw / 2, y + g.ch / 2],
          [x, y],
          [x + g.cw, y],
          [x, y + g.ch],
          [x + g.cw, y + g.ch],
        ].some(([px, py]) => insideFigures(px, py)),
      );
    }
  }
  return out;
}

/**
 * One grid's digits in their first frame: one `<text>` per brightness level,
 * each glyph placed by its own x and y, all masked to the figures. The light
 * of the monitor shades them through the fill.
 */
function digitLayer(g: Grid): El {
  const { chars, base } = cells(g);
  const level = levels(g, base, firstDrops(g));
  const covered = coveredCells(g);
  const byLevel: { xs: number[]; ys: number[]; text: string }[] = LEVELS.map(() => ({ xs: [], ys: [], text: "" }));
  for (let row = 0; row < g.rows; row++) {
    for (let col = 0; col < g.cols; col++) {
      const i = row * g.cols + col;
      if (!covered[i]) continue;
      const bucket = byLevel[level[i]];
      bucket.xs.push(r1(cellX(g, col)));
      bucket.ys.push(r1(cellBaseline(g, row)));
      bucket.text += chars[i];
    }
  }
  return el(
    "g",
    {
      // Fine digits on a wide picture, bigger ones below 40rem (the frame is a size container).
      class: g.name === "wide" ? "@max-[40rem]:hidden" : "hidden @max-[40rem]:inline",
      "data-grid": g.name,
      mask: "url(#ab-fig)",
      "font-size": g.font,
      "text-anchor": "middle",
    },
    byLevel
      .map((b, l) =>
        b.text
          ? el(
              "text",
              l === HEAD
                ? { x: b.xs.join(" "), y: b.ys.join(" "), fill: MINT }
                : { x: b.xs.join(" "), y: b.ys.join(" "), fill: "url(#ab-light)", "fill-opacity": LEVELS[l] },
              [b.text],
            )
          : null,
      )
      .filter((e): e is El => e !== null),
  );
}

/**
 * The whole picture. `title` and `desc` are its text for screen readers;
 * `mark` is the Navree mark's paths (on a 32-unit grid), shown small on the
 * screen.
 */
export function pictureTree({ title, desc, mark }: { title: string; desc: string; mark: readonly string[] }): El {
  const [tl, tr, br, bl] = monitor.screen;
  const screen = `M${tl.join(" ")}L${tr.join(" ")}L${br.join(" ")}L${bl.join(" ")}Z`;
  const layerDefs = layers.map((layer) =>
    el(
      "g",
      { id: `ab-l-${layer.id}`, "data-halo": layer.halo },
      layer.pieces.map((d) => el("path", { d })),
    ),
  );
  // The figures for the masks: each layer after the first cuts its dark gap into what is behind it.
  const shapes = el(
    "g",
    { id: "ab-shapes" },
    layers.flatMap((layer) => [
      ...(layer.halo
        ? [el("use", { href: `#ab-l-${layer.id}`, fill: "#000", stroke: "#000", "stroke-width": layer.halo * 2, "stroke-linejoin": "round" })]
        : []),
      el("use", { href: `#ab-l-${layer.id}`, fill: "currentColor" }),
    ]).concat(
      // The backrest's mesh dims the father's back; the animation reads `data-veil` too.
      el("path", { d: backrest.d, fill: "#000", "fill-opacity": backrest.veil, "data-veil": backrest.veil }),
    ),
  );
  const maskBox = { maskUnits: "userSpaceOnUse", ...box };
  const thin = { "vector-effect": "non-scaling-stroke" };

  return el(
    "svg",
    {
      viewBox: `${VIEW.x} ${VIEW.y} ${VIEW.width} ${VIEW.height}`,
      role: "img",
      "aria-labelledby": "ab-title ab-desc",
      class: "block h-auto w-full font-mono select-none",
    },
    [
      el("title", { id: "ab-title" }, [title]),
      el("desc", { id: "ab-desc" }, [desc]),
      el("defs", {}, [
        lightGradient("ab-light", GREEN, 1),
        lightGradient("ab-tone", GREEN, 0.13),
        lightGradient("ab-rim-light", MINT, 0.7),
        el("radialGradient", { id: "ab-room", gradientUnits: "userSpaceOnUse", cx: LIGHT.x, cy: LIGHT.y, r: 460 }, [
          el("stop", { offset: 0, "stop-color": GREEN, "stop-opacity": 0.2 }),
          el("stop", { offset: 0.3, "stop-color": GREEN, "stop-opacity": 0.075 }),
          el("stop", { offset: 1, "stop-color": GREEN, "stop-opacity": 0 }),
        ]),
        el("radialGradient", { id: "ab-floor", cx: 0.5, cy: 0.5, r: 0.5 }, [
          el("stop", { offset: 0, "stop-color": GREEN, "stop-opacity": 0.14 }),
          el("stop", { offset: 1, "stop-color": GREEN, "stop-opacity": 0 }),
        ]),
        el("linearGradient", { id: "ab-screen", x1: 0, y1: 0, x2: 1, y2: 1 }, [
          el("stop", { offset: 0, "stop-color": "#11422a" }),
          el("stop", { offset: 1, "stop-color": "#08261a" }),
        ]),
        fade("ab-column", GREEN, 0.26),
        // The room fades out before the picture's edges, so it has no frame.
        el("radialGradient", { id: "ab-vignette", cx: 0.52, cy: 0.5, r: 0.56 }, [
          el("stop", { offset: 0.55, "stop-color": "#fff" }),
          el("stop", { offset: 1, "stop-color": "#000" }),
        ]),
        el("mask", { id: "ab-room-fade", ...maskBox }, [rect({ fill: "url(#ab-vignette)" })]),
        el("linearGradient", { id: "ab-floor-line", gradientUnits: "userSpaceOnUse", x1: VIEW.x, y1: 0, x2: VIEW.x + VIEW.width, y2: 0 }, [
          el("stop", { offset: 0, "stop-color": GREEN, "stop-opacity": 0 }),
          el("stop", { offset: 0.2, "stop-color": GREEN, "stop-opacity": 0.4 }),
          el("stop", { offset: 0.8, "stop-color": GREEN, "stop-opacity": 0.4 }),
          el("stop", { offset: 1, "stop-color": GREEN, "stop-opacity": 0 }),
        ]),
        el("filter", { id: "ab-blur", x: "-20%", y: "-20%", width: "140%", height: "140%" }, [el("feGaussianBlur", { stdDeviation: 8 })]),
        el("filter", { id: "ab-blur-screen", x: "-60%", y: "-60%", width: "220%", height: "220%" }, [el("feGaussianBlur", { stdDeviation: 16 })]),
        ...layerDefs,
        shapes,
        el("mask", { id: "ab-fig", ...maskBox }, [el("use", { href: "#ab-shapes", color: "#fff" })]),
        el("mask", { id: "ab-solid", ...maskBox }, [el("g", { fill: "#fff" }, layers.map((layer) => el("use", { href: `#ab-l-${layer.id}` })))]),
        el("mask", { id: "ab-rim", ...maskBox }, [
          el("use", { href: "#ab-shapes", color: "#fff" }),
          el("g", { fill: "#000", transform: "translate(-1.8 0.9)" }, layers.map((layer) => el("use", { href: `#ab-l-${layer.id}` }))),
        ]),
      ]),

      // The room: the monitor's glow on the wall and the floor, and a faint rain, all fading out at the edges.
      el("g", { mask: "url(#ab-room-fade)" }, [
        rect({ fill: "url(#ab-room)" }),
        el("ellipse", { cx: 480, cy: 557, rx: 330, ry: 24, fill: "url(#ab-floor)" }),
        // The screen's light on the desk.
        el("ellipse", { cx: LIGHT.x, cy: 334, rx: 190, ry: 16, fill: "url(#ab-floor)" }),
        el("path", { d: `M${VIEW.x} 557L${VIEW.x + VIEW.width} 557`, stroke: "url(#ab-floor-line)", "stroke-width": 1, ...thin }),
      ]),
      el(
        "g",
        { "text-anchor": "middle", fill: "url(#ab-column)", mask: "url(#ab-room-fade)" },
        backgroundRain(random(911)).map((col) =>
          el(
            "text",
            {
              x: Array.from(col.text, () => col.x).join(" "),
              y: Array.from(col.text, (_, i) => r1(col.y + i * col.step)).join(" "),
              "font-size": col.font,
            },
            [col.text],
          ),
        ),
      ),

      // The chair and the desk.
      el(
        "g",
        { fill: "none", stroke: GREEN, "stroke-linecap": "round", "stroke-linejoin": "round" },
        furniture.map((s) =>
          el("path", {
            d: s.d,
            "stroke-width": s.w,
            "stroke-opacity": r3(s.tone * 0.8),
            ...thin,
          }),
        ),
      ),

      // The monitor: its glow, the screen with code on it, the frame and the stand.
      el("path", { d: screen, fill: GREEN, opacity: 0.5, filter: "url(#ab-blur-screen)" }),
      el("path", { d: screen, fill: "url(#ab-screen)" }),
      el(
        "g",
        { stroke: GREEN, "stroke-linecap": "round" },
        screenCode().map((line) =>
          el("path", { d: `M${line.from.map(r1).join(" ")}L${line.to.map(r1).join(" ")}`, "stroke-width": line.w, "stroke-opacity": line.tone }),
        ),
      ),
      el(
        "g",
        { fill: GREEN, transform: `translate(${r1(screenMark.at[0])} ${r1(screenMark.at[1])}) scale(${r3(screenMark.size / 32)})` },
        mark.map((d) => el("path", { d })),
      ),
      el("g", { fill: "none", stroke: GREEN, "stroke-linecap": "round", "stroke-linejoin": "round" }, [
        el("path", { d: screen, "stroke-width": 1.6, "stroke-opacity": 0.95, ...thin }),
        el("path", { d: monitor.stand, "stroke-width": 1.3, "stroke-opacity": 0.55, ...thin }),
        el("path", { d: monitor.base, "stroke-width": 1.3, "stroke-opacity": 0.6, ...thin }),
      ]),

      // The family: a soft glow, a dark body that hides what is behind it, a faint tone, the light on their edges, and the digits.
      el("g", { filter: "url(#ab-blur)", opacity: 0.3 }, [rect({ fill: "url(#ab-light)", mask: "url(#ab-solid)" })]),
      rect({ fill: INK, mask: "url(#ab-solid)" }),
      rect({ fill: "url(#ab-tone)", mask: "url(#ab-fig)" }),
      // The white cloth over Navraj's joora.
      el("g", { fill: MINT, "fill-opacity": 0.42 }, cloth.map((d) => el("path", { d }))),
      rect({ fill: "url(#ab-rim-light)", mask: "url(#ab-rim)" }),
      // The still digits; hidden while the animation draws the same digits over them.
      el("g", { class: "group-data-live/picture:invisible" }, grids.map(digitLayer)),
      // The backrest's frame and mesh, over the father's back.
      el("g", { fill: "none", stroke: GREEN, "stroke-linecap": "round", "stroke-linejoin": "round" }, [
        el("path", { d: backrest.d, "stroke-width": 1.5, "stroke-opacity": 0.6, ...thin }),
        el("path", { d: backrest.mesh, "stroke-width": 1, "stroke-opacity": 0.22, ...thin }),
      ]),
    ],
  );
}

/** The tree as SVG markup, to preview the picture outside the site. */
export function toMarkup(node: El | string): string {
  if (typeof node === "string") return node.replace(/&/g, "&amp;").replace(/</g, "&lt;");
  const attrs = Object.entries(node.a ?? {})
    .filter(([, v]) => v !== undefined)
    .map(([k, v]) => ` ${k}="${String(v).replace(/"/g, "&quot;")}"`)
    .join("");
  return `<${node.t}${attrs}>${(node.c ?? []).map(toMarkup).join("")}</${node.t}>`;
}
