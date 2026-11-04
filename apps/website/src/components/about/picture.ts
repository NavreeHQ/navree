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

/**
 * The first 10 hex digits of the SHA-256 of the picture's files (in name
 * order). It goes in their URLs, so a browser that kept an earlier picture
 * fetches the new one. `scripts/postbuild.ts` stops the build when it no
 * longer matches the files, and prints the right value.
 */
export const PICTURE_VERSION = "b26d9765ed";

export const imageSrc = (width: number, ext: "avif" | "webp") => `/images/about/family-${width}.${ext}?v=${PICTURE_VERSION}`;

export type Point = readonly [x: number, y: number];

/** The monitor's glass, inside its bezel. */
export const SCREEN = { x: 707, y: 147, width: 320, height: 193 } as const;

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
export const CODE_LAYOUT = { size: 7.4, line: 11.6, top: 170, gutter: 724, left: 734, title: 157, advance: 0.6 } as const;

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
    [338, 226],
    [298, 238],
    [262, 262],
    [256, 340],
    [204, 356],
    [192, 446],
    [0, 470],
  ],
  [
    [1086, 0],
    [1376, 0],
    [1376, 452],
    [1240, 438],
    [1238, 400],
    [1162, 398],
    [1150, 330],
    [1100, 300],
    [1088, 250],
  ],
];

/** Where the screen's light comes from: the middle of the glass, at the monitor's depth. */
export const LIGHT = { x: 867, y: 244, z: 320 } as const;

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

export const ROOM_DEPTH = 600;

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
    round: 28,
    figure: true,
    poly: [
      [930, 192],
      [962, 184],
      [1002, 188],
      [1038, 202],
      [1058, 216],
      [1068, 242],
      [1070, 286],
      [1090, 334],
      [1108, 378],
      [1118, 410],
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
    round: 24,
    figure: true,
    poly: [
      [600, 197],
      [625, 195],
      [648, 205],
      [661, 230],
      [672, 262],
      [680, 300],
      [677, 338],
      [700, 360],
      [724, 382],
      [731, 440],
      [713, 470],
      [702, 520],
      [740, 560],
      [757, 610],
      [780, 660],
      [801, 690],
      [816, 717],
      [796, 721],
      [772, 693],
      [750, 642],
      [730, 602],
      [690, 592],
      [640, 590],
      [600, 560],
      [600, 420],
      [590, 340],
      [570, 300],
      [560, 268],
      [568, 235],
      [585, 210],
    ],
  },
  // The father: head, back, lap and legs.
  {
    id: "father",
    z: 40,
    round: 40,
    figure: true,
    poly: [
      [397, 80],
      [420, 68],
      [487, 67],
      [528, 92],
      [534, 122],
      [541, 150],
      [547, 186],
      [539, 206],
      [536, 238],
      [524, 256],
      [552, 272],
      [585, 292],
      [600, 330],
      [608, 390],
      [614, 430],
      [604, 470],
      [622, 505],
      [664, 522],
      [694, 540],
      [700, 580],
      [742, 600],
      [760, 640],
      [748, 700],
      [736, 768],
      [440, 768],
      [428, 700],
      [418, 640],
      [300, 600],
      [276, 520],
      [274, 420],
      [277, 336],
      [293, 288],
      [328, 253],
      [390, 222],
      [388, 200],
      [386, 150],
      [389, 110],
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
      [586, 300],
      [612, 340],
      [622, 404],
      [632, 440],
      [680, 452],
      [730, 450],
      [765, 441],
      [797, 433],
      [828, 446],
      [841, 470],
      [831, 487],
      [800, 493],
      [760, 503],
      [720, 516],
      [680, 527],
      [640, 533],
      [612, 521],
      [596, 482],
      [590, 420],
      [578, 350],
    ],
  },
  // The chair, nearest: the mesh backrest, the armrest and the front of the seat.
  {
    id: "chair",
    z: 0,
    round: 16,
    poly: [
      [208, 368],
      [214, 360],
      [262, 362],
      [335, 370],
      [420, 381],
      [434, 393],
      [428, 407],
      [440, 470],
      [452, 545],
      [462, 598],
      [476, 640],
      [480, 700],
      [472, 742],
      [444, 768],
      [298, 768],
      [268, 720],
      [254, 650],
      [238, 605],
      [236, 576],
      [244, 560],
      [238, 480],
      [220, 410],
    ],
  },
  {
    id: "armrest",
    z: -6,
    round: 8,
    poly: [
      [462, 579],
      [520, 575],
      [600, 573],
      [650, 575],
      [665, 589],
      [661, 610],
      [640, 626],
      [560, 631],
      [490, 646],
      [466, 633],
    ],
  },
  {
    id: "seat",
    z: 10,
    round: 10,
    poly: [
      [560, 672],
      [686, 668],
      [708, 696],
      [706, 730],
      [681, 751],
      [642, 768],
      [560, 768],
    ],
  },
];

/**
 * Faces, hair, hands and feet: the live digits shimmer and stream over the
 * family's clothes, which the picture draws in digits, and leave these
 * alone.
 */
export const QUIET: Point[][] = [
  // The father's head and beard.
  [
    [382, 60],
    [545, 60],
    [556, 190],
    [544, 266],
    [470, 266],
    [398, 248],
    [380, 150],
  ],
  // Navraj's patka and head.
  [
    [554, 184],
    [692, 184],
    [692, 338],
    [554, 338],
  ],
  // Navreet's head with its banded sections, and her two pigtails.
  [
    [912, 178],
    [1074, 178],
    [1074, 332],
    [912, 332],
  ],
  [
    [934, 296],
    [1006, 296],
    [1006, 538],
    [934, 538],
  ],
  [
    [1040, 288],
    [1126, 288],
    [1126, 528],
    [1040, 528],
  ],
  // Hands at the desk, and Navraj's foot.
  [
    [752, 436],
    [846, 436],
    [846, 500],
    [752, 500],
  ],
  [
    [826, 480],
    [894, 480],
    [894, 530],
    [826, 530],
  ],
  [
    [764, 640],
    [820, 640],
    [820, 724],
    [764, 724],
  ],
];

/** Where the family's heads are, kept clear of the out-of-focus digits near the camera. */
export const HEADS: Point[] = [
  [462, 160],
  [620, 255],
  [985, 245],
];
