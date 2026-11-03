// A small PNG reader for screenshots: 8-bit, not interlaced, as the browsers write
// them. Enough to look at pixels without a dependency.

import { inflateSync } from "node:zlib";

/** `{width, height, channels, data}` with `data` holding the unfiltered rows. */
export function decodePng(buffer) {
  const signature = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
  if (!signature.every((byte, i) => buffer[i] === byte)) throw new Error("not a PNG");
  let at = 8;
  let header = null;
  const parts = [];
  while (at < buffer.length) {
    const length = buffer.readUInt32BE(at);
    const type = buffer.toString("latin1", at + 4, at + 8);
    const body = buffer.subarray(at + 8, at + 8 + length);
    if (type === "IHDR") header = { width: body.readUInt32BE(0), height: body.readUInt32BE(4), depth: body[8], color: body[9], interlace: body[12] };
    if (type === "IDAT") parts.push(body);
    if (type === "IEND") break;
    at += 12 + length;
  }
  if (!header || header.depth !== 8 || header.interlace !== 0) throw new Error("unsupported PNG");
  const channels = { 0: 1, 2: 3, 4: 2, 6: 4 }[header.color];
  if (!channels) throw new Error(`unsupported PNG colour type ${header.color}`);
  const raw = inflateSync(Buffer.concat(parts));
  const stride = header.width * channels;
  const data = Buffer.alloc(stride * header.height);
  for (let y = 0; y < header.height; y += 1) {
    const filter = raw[y * (stride + 1)];
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    for (let x = 0; x < stride; x += 1) {
      const left = x >= channels ? data[y * stride + x - channels] : 0;
      const up = y > 0 ? data[(y - 1) * stride + x] : 0;
      const upLeft = y > 0 && x >= channels ? data[(y - 1) * stride + x - channels] : 0;
      let value = line[x];
      if (filter === 1) value += left;
      else if (filter === 2) value += up;
      else if (filter === 3) value += (left + up) >> 1;
      else if (filter === 4) {
        const p = left + up - upLeft;
        const pa = Math.abs(p - left);
        const pb = Math.abs(p - up);
        const pc = Math.abs(p - upLeft);
        value += pa <= pb && pa <= pc ? left : pb <= pc ? up : upLeft;
      }
      data[y * stride + x] = value & 255;
    }
  }
  return { width: header.width, height: header.height, channels, data };
}

/** The colour at a pixel as `rrggbb` (alpha ignored). */
export function pixel(image, x, y) {
  const at = (y * image.width + x) * image.channels;
  const [r, g, b] = image.channels >= 3 ? [image.data[at], image.data[at + 1], image.data[at + 2]] : [image.data[at], image.data[at], image.data[at]];
  return [r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("");
}

/** How many different colours a box of the image holds, sampled on a grid. */
export function colours(image, x, y, width, height, steps = 12) {
  const seen = new Set();
  for (let i = 0; i < steps; i += 1) {
    for (let j = 0; j < steps; j += 1) {
      seen.add(pixel(image, Math.min(image.width - 1, Math.floor(x + (width * (i + 0.5)) / steps)), Math.min(image.height - 1, Math.floor(y + (height * (j + 0.5)) / steps))));
    }
  }
  return seen.size;
}
