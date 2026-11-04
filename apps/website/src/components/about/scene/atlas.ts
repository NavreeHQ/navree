/**
 * The glyph atlas of the live picture: one texture holding every glyph it
 * draws, so all of them go to the GPU in a single draw call.
 *
 * A 16 x 8 grid of square cells. Each cell keeps three versions of its
 * glyph in the three colour channels: red is the sharp glyph, green a soft
 * glow around it, blue a heavy blur (for the out-of-focus digits near the
 * camera). The shader mixes the three per glyph.
 *
 * - cells 0 and 1: the digits "0" and "1";
 * - cells 2 to 96: printable ASCII (code 32 to 126), for the code on the screen;
 * - `CELL.glow`: a round soft light; `CELL.caret`: the text cursor.
 */

/** Cells across and down. */
export const GRID = 16;
export const ROWS = 8;
/** Pixels per cell side. */
export const CELL_PX = 64;
/** How tall a digit stands in its cell (0 to 1): a glyph drawn `h` tall needs a quad `h / DIGIT` tall. */
export const DIGIT = 0.4;
/** The same for the screen's text, measured on a capital letter. */
export const TEXT = 0.36;

export const CELL = { zero: 0, one: 1, glow: 98, caret: 99 } as const;

/** The cell of a printable ASCII character (a space for anything else). */
export function charCell(ch: string): number {
  const code = ch.charCodeAt(0);
  return 2 + (code >= 32 && code <= 126 ? code - 32 : 0);
}

/**
 * Draws the atlas on a new canvas. `font` is the CSS font family of the
 * digits and text. Each blur is its own step, with `pause` between them, so
 * none of them blocks the page for long.
 */
export async function drawAtlas(font: string, pause: () => Promise<void>): Promise<HTMLCanvasElement | null> {
  const width = GRID * CELL_PX;
  const height = ROWS * CELL_PX;
  const sharp = document.createElement("canvas");
  sharp.width = width;
  sharp.height = height;
  const s = sharp.getContext("2d");
  if (!s) return null;

  // Size the font so a digit stands DIGIT of a cell tall.
  s.font = `100px ${font}`;
  const zero = s.measureText("0");
  const digitPx = (DIGIT * CELL_PX * 100) / Math.max(1, zero.actualBoundingBoxAscent + zero.actualBoundingBoxDescent);
  s.font = `${digitPx}px ${font}`;
  const m = s.measureText("0");
  s.fillStyle = "#fff";
  s.textAlign = "center";
  const cellXY = (i: number) => [(i % GRID) * CELL_PX, Math.floor(i / GRID) * CELL_PX] as const;
  ["0", "1"].forEach((digit, i) => {
    const [x, y] = cellXY(i);
    s.fillText(digit, x + CELL_PX / 2, y + CELL_PX / 2 + (m.actualBoundingBoxAscent - m.actualBoundingBoxDescent) / 2);
  });

  // The screen's text: one baseline for every character, so lines of code sit straight.
  s.font = `100px ${font}`;
  const cap = s.measureText("H").actualBoundingBoxAscent;
  const textPx = (TEXT * CELL_PX * 100) / Math.max(1, cap);
  s.font = `${textPx}px ${font}`;
  const capPx = s.measureText("H").actualBoundingBoxAscent;
  for (let code = 33; code <= 126; code++) {
    const [x, y] = cellXY(2 + code - 32);
    s.fillText(String.fromCharCode(code), x + CELL_PX / 2, y + CELL_PX / 2 + capPx / 2);
  }

  // The caret: a slim block the height of a capital letter.
  {
    const [x, y] = cellXY(CELL.caret);
    s.fillRect(x + CELL_PX * 0.36, y + CELL_PX / 2 - capPx / 2 - 2, CELL_PX * 0.28, capPx + 4);
  }

  const out = document.createElement("canvas");
  out.width = width;
  out.height = height;
  const o = out.getContext("2d");
  const tint = document.createElement("canvas");
  tint.width = width;
  tint.height = height;
  const t = tint.getContext("2d");
  if (!o || !t) return null;
  o.fillStyle = "#000";
  o.fillRect(0, 0, width, height);
  o.globalCompositeOperation = "lighter";
  // Each version goes through `tint`, coloured into its channel, then added to the atlas.
  const channel = (color: string, blur: number) => {
    t.globalCompositeOperation = "source-over";
    t.clearRect(0, 0, width, height);
    t.filter = blur ? `blur(${blur}px)` : "none";
    t.drawImage(sharp, 0, 0);
    if (blur) t.drawImage(sharp, 0, 0);
    t.filter = "none";
    t.globalCompositeOperation = "source-in";
    t.fillStyle = color;
    t.fillRect(0, 0, width, height);
    o.drawImage(tint, 0, 0);
  };
  channel("#f00", 0);
  await pause();
  channel("#0f0", 4.5);
  await pause();
  channel("#00f", 9);

  // The soft light: the same radial falloff in all three channels.
  {
    const [x, y] = cellXY(CELL.glow);
    const g = o.createRadialGradient(x + CELL_PX / 2, y + CELL_PX / 2, 0, x + CELL_PX / 2, y + CELL_PX / 2, CELL_PX / 2 - 1);
    g.addColorStop(0, "rgba(255,255,255,1)");
    g.addColorStop(0.25, "rgba(255,255,255,0.55)");
    g.addColorStop(0.55, "rgba(255,255,255,0.18)");
    g.addColorStop(1, "rgba(255,255,255,0)");
    o.globalCompositeOperation = "source-over";
    o.fillStyle = "#000";
    o.fillRect(x, y, CELL_PX, CELL_PX);
    o.fillStyle = g;
    o.fillRect(x, y, CELL_PX, CELL_PX);
  }
  sharp.width = sharp.height = 0;
  tint.width = tint.height = 0;
  return out;
}
