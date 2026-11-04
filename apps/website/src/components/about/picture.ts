/**
 * The about picture as data: the image files, where its screen is, the
 * code shown on that screen, and the picture's depth, traced by hand as
 * polygons. Both the still (`family-picture.tsx`) and the live scene
 * (`scene/`) read it, so they share one composition.
 *
 * Coordinates are the picture's own pixels at 1376 x 768, top left origin.
 * Depth (`z`) is in the same units, growing away from the viewer: the chair
 * is nearest, then the father, his son, his daughter, the desk and monitor,
 * and the room behind.
 */

export const IMAGE = { width: 1376, height: 768 } as const;

/** The picture's widths on disk, as AVIF and WebP: `/images/about/family-<width>.<ext>`. */
export const WIDTHS = [640, 1280, 1920, 2560] as const;
export const imageSrc = (width: number, ext: "avif" | "webp") => `/images/about/family-${width}.${ext}`;

export type Point = readonly [x: number, y: number];

/** The monitor's glass, inside its bezel. */
export const SCREEN = { x: 706, y: 146, width: 331, height: 201 } as const;

/** Navreet's head, which stands in front of the screen's right part: the code never covers it. */
export const HEAD_OVER_SCREEN: Point[] = [
  [921, 352],
  [921, 300],
  [919, 262],
  [922, 230],
  [938, 206],
  [965, 190],
  [990, 187],
  [1020, 196],
  [1045, 222],
  [1056, 262],
  [1058, 320],
  [1060, 352],
];

/** The code on the screen: a Navree controller and its module, from the modules guide. */
export const CODE = [
  '#[controller("/deals", version = "1")]',
  "impl DealsController {",
  '    #[get("/")]',
  "    #[authenticated]",
  "    async fn index(&self)",
  "        -> Json<Vec<String>> {",
  "        Json(self.deals.titles().await)",
  "    }",
  "}",
  "",
  "#[module(",
  "    providers = [DealService],",
  "    controllers = [DealsController],",
  ")]",
  "pub struct DealsModule;",
] as const;

/** The file name over the code. */
export const CODE_TITLE = "src/modules/deals/mod.rs";

/** How the code is set on the screen: font size, line height, the first baseline, and the left edges of the line numbers and the code. */
export const CODE_LAYOUT = { size: 7.6, line: 12, top: 172, gutter: 724, left: 734, title: 158, advance: 0.6 } as const;

/** Where line `i` of the code sits (its baseline), and the x of column `col`. */
export const codeBaseline = (i: number) => CODE_LAYOUT.top + i * CODE_LAYOUT.line;
export const codeX = (col: number) => CODE_LAYOUT.left + col * CODE_LAYOUT.size * CODE_LAYOUT.advance;

/**
 * The digit rain the picture has at its left and right edges. The live
 * scene hides it there and lets its own rain fall in its place.
 */
export const RAIN_ZONES: Point[][] = [
  [
    [0, 0],
    [338, 0],
    [338, 250],
    [292, 262],
    [286, 340],
    [204, 356],
    [192, 446],
    [0, 470],
  ],
  [
    [1074, 0],
    [1376, 0],
    [1376, 452],
    [1240, 438],
    [1238, 400],
    [1162, 398],
    [1154, 350],
    [1098, 346],
    [1078, 300],
  ],
];

/** Where the screen's light comes from: the middle of the glass, at the monitor's depth. */
export const LIGHT = { x: 872, y: 246, z: 320 } as const;

/**
 * A layer of depth: a polygon at depth `z`. `round` swells a figure towards
 * the viewer by up to that much in its middle; `slope` tilts the depth
 * along x and y (depth units a pixel), for a surface that recedes, such as
 * the desk's top or an arm reaching forward. Layers are listed back to
 * front: a later one wins where they overlap.
 */
export type DepthLayer = {
  id: string;
  z: number;
  round?: number;
  slope?: readonly [number, number];
  /** Where the slope is measured from (defaults to the polygon's first point). */
  at?: Point;
  poly: Point[];
  /** A figure, whose digits shimmer and stream once the picture is alive. */
  figure?: boolean;
};

export const ROOM_DEPTH = 900;

export const DEPTH_LAYERS: DepthLayer[] = [
  // The desk: its top recedes from the front edge to the wall; legs; keyboard; monitor; plant and mug.
  {
    id: "desk-top",
    z: 300,
    slope: [0, -1.6],
    at: [700, 446],
    poly: [
      [192, 444],
      [700, 438],
      [1300, 456],
      [1304, 472],
      [1086, 530],
      [204, 474],
    ],
  },
  { id: "desk-leg-left", z: 190, poly: [[204, 462], [246, 462], [246, 768], [204, 768]] },
  { id: "desk-leg-mid", z: 180, poly: [[1068, 518], [1094, 518], [1094, 768], [1068, 768]] },
  { id: "desk-leg-right", z: 230, poly: [[1258, 462], [1286, 462], [1286, 768], [1258, 768]] },
  { id: "monitor", z: 320, poly: [[696, 132], [1044, 132], [1044, 372], [900, 372], [895, 448], [790, 448], [795, 372], [696, 372]] },
  { id: "keyboard", z: 240, poly: [[694, 438], [900, 438], [904, 482], [690, 482]] },
  { id: "plant-mug", z: 290, poly: [[1090, 362], [1162, 360], [1162, 408], [1236, 408], [1236, 482], [1090, 482]] },
  // Navreet, standing at the desk on the right, a hand reaching to it.
  {
    id: "navreet",
    z: 150,
    round: 40,
    figure: true,
    poly: [
      [930, 192],
      [962, 186],
      [1002, 191],
      [1036, 205],
      [1052, 232],
      [1056, 266],
      [1058, 304],
      [1064, 330],
      [1074, 362],
      [1094, 396],
      [1114, 406],
      [1128, 442],
      [1138, 482],
      [1144, 516],
      [1128, 530],
      [1108, 524],
      [1098, 548],
      [1090, 604],
      [1094, 664],
      [1100, 724],
      [1104, 768],
      [870, 768],
      [878, 700],
      [888, 640],
      [894, 580],
      [898, 546],
      [886, 526],
      [858, 524],
      [832, 514],
      [830, 498],
      [850, 487],
      [870, 478],
      [884, 456],
      [898, 420],
      [904, 398],
      [914, 360],
      [924, 330],
      [918, 300],
      [915, 262],
      [918, 226],
    ],
  },
  // Navraj, on his father's lap: his patka and head, his back, and a leg reaching down past the chair.
  {
    id: "navraj",
    z: 110,
    round: 34,
    figure: true,
    poly: [
      [602, 192],
      [625, 187],
      [648, 197],
      [656, 222],
      [668, 240],
      [677, 262],
      [683, 292],
      [681, 318],
      [677, 336],
      [688, 366],
      [700, 410],
      [706, 442],
      [700, 470],
      [722, 520],
      [754, 546],
      [774, 578],
      [792, 650],
      [810, 700],
      [814, 718],
      [794, 720],
      [770, 690],
      [750, 622],
      [736, 588],
      [700, 582],
      [640, 586],
      [580, 586],
      [572, 540],
      [575, 480],
      [578, 420],
      [584, 360],
      [588, 322],
      [570, 296],
      [560, 270],
      [566, 240],
      [585, 214],
    ],
  },
  // The father: head, back, lap and legs.
  {
    id: "father",
    z: 40,
    round: 70,
    figure: true,
    poly: [
      [398, 88],
      [430, 70],
      [478, 63],
      [522, 75],
      [537, 98],
      [534, 120],
      [542, 146],
      [549, 186],
      [541, 206],
      [538, 236],
      [524, 256],
      [532, 268],
      [562, 300],
      [548, 362],
      [562, 430],
      [580, 500],
      [600, 560],
      [662, 574],
      [722, 584],
      [752, 600],
      [762, 642],
      [748, 700],
      [732, 768],
      [440, 768],
      [428, 700],
      [418, 622],
      [300, 560],
      [292, 450],
      [292, 350],
      [300, 300],
      [330, 262],
      [372, 247],
      [398, 236],
      [392, 200],
      [384, 150],
      [387, 110],
    ],
  },
  // His right arm, across his son to the keyboard: it reaches forward as it goes.
  {
    id: "father-arm",
    z: 60,
    round: 26,
    slope: [0.45, 0],
    at: [560, 400],
    figure: true,
    poly: [
      [556, 298],
      [598, 320],
      [616, 382],
      [628, 440],
      [680, 455],
      [740, 452],
      [775, 442],
      [806, 445],
      [829, 458],
      [839, 478],
      [821, 491],
      [790, 493],
      [760, 500],
      [720, 516],
      [680, 526],
      [640, 531],
      [607, 516],
      [584, 490],
      [564, 430],
      [544, 360],
    ],
  },
  // The chair, nearest: the mesh backrest, the armrest and the front of the seat.
  {
    id: "chair",
    z: 0,
    round: 16,
    poly: [
      [206, 370],
      [214, 361],
      [262, 363],
      [332, 370],
      [418, 379],
      [429, 392],
      [421, 405],
      [433, 470],
      [446, 560],
      [458, 598],
      [470, 640],
      [476, 700],
      [470, 742],
      [442, 768],
      [298, 768],
      [270, 720],
      [256, 650],
      [242, 610],
      [236, 576],
      [246, 560],
      [236, 482],
      [220, 410],
    ],
  },
  {
    id: "armrest",
    z: -6,
    round: 8,
    poly: [
      [452, 597],
      [520, 585],
      [600, 575],
      [652, 571],
      [667, 584],
      [663, 601],
      [640, 613],
      [560, 623],
      [490, 641],
      [468, 631],
    ],
  },
  {
    id: "seat",
    z: 10,
    round: 10,
    poly: [
      [560, 688],
      [690, 679],
      [713, 699],
      [709, 731],
      [681, 751],
      [642, 768],
      [560, 768],
    ],
  },
];

/** Where the family's heads are, kept clear of the out-of-focus digits near the camera. */
export const HEADS: Point[] = [
  [462, 160],
  [620, 255],
  [985, 245],
];
