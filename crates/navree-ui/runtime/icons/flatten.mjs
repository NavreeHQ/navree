// Takes the references out of a flag's artwork, so that every flag is shapes and
// nothing that points at another element.
//
// An SVG that says `url(#a)` or `href="#a"` depends on whichever element in the
// document has the id `a` first. Inline in a page, that element can be in a copy of
// the same flag inside a closed menu, a hidden tab or a closed dialog, where
// browsers do not draw clips, markers or gradients: the flag in view loses its stars
// or its coat of arms. So the references are resolved here, once:
//
// - `use` stays: a `use` of an element in a hidden copy of the flag is drawn, like a
//   sprite. (Where a clip is inside what a `use` names, the `use` is copied instead.)
// - A clip that covers the whole flag is dropped; a clip that is a smaller rectangle
//   becomes a nested <svg>, which clips to its box.
// - A marker drawn at the vertices of a path becomes a copy of the marker at each
//   vertex.
// - A gradient becomes the colour it averages to. This is the one loss: seven coats
//   of arms are shaded by gradients, and at the sizes a flag is drawn they read the
//   same flat.
//
// Anything it cannot resolve exactly (a curved clip, a marker on a curve, a colour
// name it does not know) is an error, so a new release of the artwork is looked at
// by a person.

const NUMBER = /[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?/g;

const NAMED = {
  white: [255, 255, 255], black: [0, 0, 0], red: [255, 0, 0], lime: [0, 255, 0], blue: [0, 0, 255],
  yellow: [255, 255, 0], gray: [128, 128, 128], grey: [128, 128, 128], green: [0, 128, 0],
  orange: [255, 165, 0], navy: [0, 0, 128], maroon: [128, 0, 0], silver: [192, 192, 192],
};

function clone(node) {
  return { tag: node.tag, attrs: node.attrs.map(([k, v]) => [k, v]), children: node.children.map(clone) };
}

const attr = (node, name) => node.attrs.find(([key]) => key === name)?.[1];
const hasAttr = (node, name) => node.attrs.some(([key]) => key === name);
function setAttr(node, name, value) {
  const found = node.attrs.find(([key]) => key === name);
  if (found) found[1] = value;
  else node.attrs.push([name, value]);
}
const dropAttr = (node, name) => {
  node.attrs = node.attrs.filter(([key]) => key !== name);
};
const refOf = (node) => {
  const value = attr(node, "href") ?? attr(node, "xlink:href");
  const match = /^#([A-Za-z0-9_-]+)$/.exec(value ?? "");
  return match ? match[1] : null;
};

function walk(node, visit) {
  visit(node);
  node.children.forEach((child) => walk(child, visit));
}

/** A clip that cannot be resolved while a `use` still points at the element it sits in. */
export class NeedsInlineUses extends Error {}

/** Every element with an id, by id. */
function idMap(root) {
  const ids = new Map();
  walk(root, (node) => {
    const id = attr(node, "id");
    if (id) ids.set(id, node);
  });
  return ids;
}

// ---------------------------------------------------------------- colours

function parseColour(text, where) {
  const value = text.trim();
  const hex = /^#([0-9a-fA-F]{3,8})$/.exec(value);
  if (hex) {
    let h = hex[1];
    if (h.length === 3 || h.length === 4) h = [...h].map((c) => c + c).join("");
    if (h.length === 6 || h.length === 8) {
      return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16), h.length === 8 ? parseInt(h.slice(6, 8), 16) / 255 : 1];
    }
  }
  const named = NAMED[value.toLowerCase()];
  if (named) return [...named, 1];
  throw new Error(`${where}: the colour ${JSON.stringify(value)} is not one the generator knows`);
}

function offsetOf(text, where) {
  const value = String(text ?? "0").trim();
  const number = parseFloat(value);
  if (!Number.isFinite(number)) throw new Error(`${where}: odd stop offset ${value}`);
  return Math.min(1, Math.max(0, value.endsWith("%") ? number / 100 : number));
}

/** The stops of a gradient, following `href` to the gradient it borrows them from. */
function stopsOf(gradient, ids, where, depth = 0) {
  const own = gradient.children.filter((child) => child.tag === "stop");
  if (own.length) return own;
  const target = refOf(gradient);
  if (!target || depth > 8) throw new Error(`${where}: a gradient with no stops`);
  const next = ids.get(target);
  if (!next) throw new Error(`${where}: a gradient borrows from #${target}, which is not there`);
  return stopsOf(next, ids, where, depth + 1);
}

/** The colour a gradient averages to along its length, and its average opacity. */
function averageColour(gradient, ids, where) {
  const stops = stopsOf(gradient, ids, where).map((stop) => {
    const [r, g, b, a] = parseColour(attr(stop, "stop-color") ?? "#000000", where);
    const opacity = attr(stop, "stop-opacity");
    return { offset: offsetOf(attr(stop, "offset"), where), rgb: [r, g, b], alpha: a * (opacity === undefined ? 1 : parseFloat(opacity)) };
  });
  stops.sort((a, b) => a.offset - b.offset);
  // The area under each channel along 0 to 1: flat before the first stop and after the last.
  const channels = [0, 0, 0, 0];
  const add = (width, from, to) => {
    for (let i = 0; i < 3; i += 1) channels[i] += (width * (from.rgb[i] + to.rgb[i])) / 2;
    channels[3] += (width * (from.alpha + to.alpha)) / 2;
  };
  add(stops[0].offset, stops[0], stops[0]);
  for (let i = 0; i + 1 < stops.length; i += 1) add(stops[i + 1].offset - stops[i].offset, stops[i], stops[i + 1]);
  add(1 - stops.at(-1).offset, stops.at(-1), stops.at(-1));
  const hex = channels.slice(0, 3).map((c) => Math.round(c).toString(16).padStart(2, "0")).join("");
  return { colour: `#${hex}`, opacity: Math.round(channels[3] * 1000) / 1000 };
}

// ------------------------------------------------------------------ paths

/**
 * The vertices of a path made only of straight moves, in absolute coordinates,
 * or null when it has a curve or an arc.
 */
function vertices(d) {
  const tokens = d.match(/[A-Za-z]|[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?/g) ?? [];
  const points = [];
  let x = 0;
  let y = 0;
  let startX = 0;
  let startY = 0;
  let command = null;
  let at = 0;
  const number = () => {
    const value = parseFloat(tokens[at]);
    if (!Number.isFinite(value)) throw new Error(`a path with a number that is not one: ${tokens[at]}`);
    at += 1;
    return value;
  };
  while (at < tokens.length) {
    if (/^[A-Za-z]$/.test(tokens[at])) {
      command = tokens[at];
      at += 1;
      if (command === "Z" || command === "z") {
        x = startX;
        y = startY;
        points.push([x, y]);
        continue;
      }
    } else if (command === null) {
      return null;
    }
    switch (command) {
      case "M": case "m": case "L": case "l": {
        const nx = number();
        const ny = number();
        const relative = command === "m" || command === "l";
        x = relative ? x + nx : nx;
        y = relative ? y + ny : ny;
        if (command === "M" || command === "m") {
          startX = x;
          startY = y;
          command = command === "M" ? "L" : "l";
        }
        points.push([x, y]);
        break;
      }
      case "H": x = number(); points.push([x, y]); break;
      case "h": x += number(); points.push([x, y]); break;
      case "V": y = number(); points.push([x, y]); break;
      case "v": y += number(); points.push([x, y]); break;
      default:
        return null;
    }
  }
  return points;
}

/** `{x, y, width, height}` of a clip shape that is an axis-aligned rectangle, else null. */
function rectangle(shape) {
  if (shape.tag === "rect") {
    if (hasAttr(shape, "transform")) return null;
    const [x, y, width, height] = ["x", "y", "width", "height"].map((name) => parseFloat(attr(shape, name) ?? "0"));
    // A corner rounded by a fiftieth of the side is a corner.
    for (const [radius, side] of [["rx", width], ["ry", height]]) {
      if (parseFloat(attr(shape, radius) ?? "0") > side * 0.02) return null;
    }
    return { x, y, width, height };
  }
  if (shape.tag !== "path" || hasAttr(shape, "transform")) return null;
  const points = vertices(attr(shape, "d") ?? "");
  if (!points || points.length < 4) return null;
  const xs = points.map(([px]) => px);
  const ys = points.map(([, py]) => py);
  // Every edge must run along an axis, or the shape is not a rectangle.
  for (let i = 1; i < points.length; i += 1) {
    const [ax, ay] = points[i - 1];
    const [bx, by] = points[i];
    // The closing edge of a drawn rectangle can be off by a tenth of a unit.
    if (Math.abs(ax - bx) > 0.25 && Math.abs(ay - by) > 0.25) return null;
  }
  const x = Math.min(...xs);
  const y = Math.min(...ys);
  return { x, y, width: Math.max(...xs) - x, height: Math.max(...ys) - y };
}

// --------------------------------------------------------------- the passes

/** `use` becomes a group holding a copy of what it names. */
function inlineUses(root, where) {
  const ids = idMap(root);
  const expand = (node, depth) => {
    if (depth > 8) throw new Error(`${where}: uses nested too deep`);
    node.children = node.children.map((child) => {
      if (child.tag !== "use") {
        expand(child, depth);
        return child;
      }
      const target = ids.get(refOf(child));
      if (!target) throw new Error(`${where}: a use of ${refOf(child)}, which is not there`);
      if (["svg", "symbol"].includes(target.tag)) throw new Error(`${where}: a use of <${target.tag}>`);
      const copy = clone(target);
      dropAttr(copy, "id");
      const holder = { children: [copy] };
      expand(holder, depth + 1);
      const group = { tag: "g", attrs: [], children: holder.children };
      let transform = attr(child, "transform");
      const x = parseFloat(attr(child, "x") ?? "0");
      const y = parseFloat(attr(child, "y") ?? "0");
      if (x || y) transform = `${transform ? `${transform} ` : ""}translate(${x} ${y})`;
      if (transform) group.attrs.push(["transform", transform]);
      for (const [name, value] of child.attrs) {
        if (["href", "xlink:href", "x", "y", "width", "height", "transform", "id"].includes(name)) continue;
        group.attrs.push([name, value]);
      }
      return group;
    });
  };
  expand(root, 0);
}

/** Gradients become the flat colour they average to. */
function flattenGradients(root, where) {
  const ids = idMap(root);
  const flattened = [];
  walk(root, (node) => {
    for (const name of ["fill", "stroke"]) {
      const match = /^url\(#([A-Za-z0-9_-]+)\)$/.exec(attr(node, name) ?? "");
      if (!match) continue;
      const gradient = ids.get(match[1]);
      if (!gradient || !["linearGradient", "radialGradient"].includes(gradient.tag)) {
        throw new Error(`${where}: ${name} paints with #${match[1]}, which is not a gradient`);
      }
      const { colour, opacity } = averageColour(gradient, ids, where);
      setAttr(node, name, colour);
      if (opacity < 1) {
        const key = `${name}-opacity`;
        setAttr(node, key, String(Math.round(parseFloat(attr(node, key) ?? "1") * opacity * 1000) / 1000));
      }
      flattened.push(match[1]);
    }
  });
  return flattened.length;
}

// ------------------------------------------------------------- transforms

const IDENTITY = [1, 0, 0, 1, 0, 0];

/** a * b, both [a, b, c, d, e, f] as in SVG's matrix(). */
function multiply(m, n) {
  return [
    m[0] * n[0] + m[2] * n[1], m[1] * n[0] + m[3] * n[1],
    m[0] * n[2] + m[2] * n[3], m[1] * n[2] + m[3] * n[3],
    m[0] * n[4] + m[2] * n[5] + m[4], m[1] * n[4] + m[3] * n[5] + m[5],
  ];
}

/** A transform attribute as one matrix. */
function matrixOf(text, where) {
  let matrix = IDENTITY;
  const list = /([A-Za-z]+)\s*\(([^)]*)\)/g;
  let rest = text.trim();
  for (const match of text.matchAll(list)) {
    const args = (match[2].match(/[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?/g) ?? []).map(Number);
    let next;
    switch (match[1]) {
      case "translate": next = [1, 0, 0, 1, args[0] ?? 0, args[1] ?? 0]; break;
      case "scale": next = [args[0] ?? 1, 0, 0, args[1] ?? args[0] ?? 1, 0, 0]; break;
      case "matrix":
        if (args.length !== 6) throw new Error(`${where}: a matrix of ${args.length} numbers`);
        next = args;
        break;
      case "rotate": {
        const r = ((args[0] ?? 0) * Math.PI) / 180;
        const [cx, cy] = [args[1] ?? 0, args[2] ?? 0];
        const turn = [Math.cos(r), Math.sin(r), -Math.sin(r), Math.cos(r), 0, 0];
        next = multiply(multiply([1, 0, 0, 1, cx, cy], turn), [1, 0, 0, 1, -cx, -cy]);
        break;
      }
      default:
        throw new Error(`${where}: the transform ${match[1]} is not handled`);
    }
    matrix = multiply(matrix, next);
    rest = rest.replace(match[0], "");
  }
  if (rest.replace(/[\s,]/g, "")) throw new Error(`${where}: a transform that cannot be read: ${text}`);
  return matrix;
}

/** The point `p` mapped back through `m`. */
function inverseApply(m, [x, y]) {
  const det = m[0] * m[3] - m[1] * m[2];
  if (Math.abs(det) < 1e-12) throw new Error("a transform that cannot be undone");
  const px = x - m[4];
  const py = y - m[5];
  return [(m[3] * px - m[2] * py) / det, (-m[1] * px + m[0] * py) / det];
}

/**
 * Clips that are rectangles. One that covers the whole flag is dropped. A smaller one
 * becomes a nested <svg> of that size, inside the element's own transform, which
 * clips what it holds to its box.
 */
function inlineClips(root, viewBox, where, kept, copyUses) {
  const ids = idMap(root);
  const [vx, vy, vw, vh] = viewBox.split(/[\s,]+/).map(Number);
  const corners = [[vx, vy], [vx + vw, vy], [vx, vy + vh], [vx + vw, vy + vh]];
  const wrap = (parent, outer, inDefs = false) => {
    parent.children = parent.children.map((child) => {
      if (inDefs && !copyUses && hasAttr(child, "clip-path")) throw new NeedsInlineUses(`${where}: a clip inside what a use names`);
      const own = hasAttr(child, "transform") ? matrixOf(attr(child, "transform"), where) : IDENTITY;
      const matrix = multiply(outer, own);
      wrap(child, matrix, inDefs || child.tag === "defs");
      const match = /^url\(#([A-Za-z0-9_-]+)\)$/.exec(attr(child, "clip-path") ?? "");
      if (!match) return child;
      const clip = ids.get(match[1]);
      if (!clip || clip.tag !== "clipPath") throw new Error(`${where}: clip-path #${match[1]} is not a clipPath`);
      if (hasAttr(clip, "transform") || attr(clip, "clipPathUnits") === "objectBoundingBox") throw new Error(`${where}: a clipPath with its own units`);
      if (clip.children.length !== 1) throw new Error(`${where}: a clipPath of ${clip.children.length} shapes`);
      const box = rectangle(clip.children[0]);
      if (!box) {
        // Kept: a polygon cannot be written without a clip. Its id is made unique to the flag later.
        kept.add(match[1]);
        return child;
      }
      // The flag's corners, in the coordinates the clip is written in: a convex box
      // holds the flag when it holds every corner.
      const covers = corners.map((corner) => inverseApply(matrix, corner)).every(([x, y]) =>
        x >= box.x - 1e-6 && x <= box.x + box.width + 1e-6 && y >= box.y - 1e-6 && y <= box.y + box.height + 1e-6);
      dropAttr(child, "clip-path");
      if (covers) return child;
      // Split the element: its transform stays outside, the clip goes inside it.
      const transform = attr(child, "transform");
      dropAttr(child, "transform");
      const nested = {
        tag: "svg",
        attrs: [["x", String(box.x)], ["y", String(box.y)], ["width", String(box.width)], ["height", String(box.height)], ["viewBox", `${box.x} ${box.y} ${box.width} ${box.height}`]],
        children: [child],
      };
      return transform ? { tag: "g", attrs: [["transform", transform]], children: [nested] } : nested;
    });
  };
  wrap(root, IDENTITY);
}

/** A marker at the vertices of a straight-sided path becomes a copy of the marker at each one. */
function inlineMarkers(root, where) {
  const ids = idMap(root);
  const place = (parent) => {
    parent.children = parent.children.map((child) => {
      place(child);
      for (const name of ["marker-start", "marker-end"]) {
        if (hasAttr(child, name)) throw new Error(`${where}: ${name} is not handled`);
      }
      const match = /^url\(#([A-Za-z0-9_-]+)\)$/.exec(attr(child, "marker-mid") ?? "");
      if (!match) return child;
      const marker = ids.get(match[1]);
      if (!marker || marker.tag !== "marker") throw new Error(`${where}: marker-mid is not a marker`);
      if (child.tag !== "path") throw new Error(`${where}: marker-mid on <${child.tag}>`);
      if (hasAttr(marker, "viewBox") || hasAttr(marker, "orient") || hasAttr(marker, "refX") || hasAttr(marker, "refY") || (hasAttr(marker, "markerUnits") && attr(marker, "markerUnits") !== "strokeWidth")) {
        throw new Error(`${where}: a marker with options the generator does not handle`);
      }
      if (hasAttr(child, "transform")) throw new Error(`${where}: a marker on a path that has a transform`);
      const points = vertices(attr(child, "d") ?? "");
      if (!points) throw new Error(`${where}: a marker on a path that is not straight`);
      const strokeWidth = parseFloat(attr(child, "stroke-width") ?? "1");
      const group = { tag: "g", attrs: [], children: [] };
      // The host is drawn first. With no stroke it draws nothing, only carries the markers.
      const stroked = attr(child, "stroke") && attr(child, "stroke") !== "none";
      const host = clone(child);
      dropAttr(host, "marker-mid");
      if (stroked) group.children.push(host);
      for (const [x, y] of points.slice(1, -1)) {
        const copy = { tag: "g", attrs: [["transform", `translate(${round(x)} ${round(y)}) scale(${strokeWidth})`]], children: marker.children.map(clone) };
        group.children.push(copy);
      }
      return group;
    });
  };
  place(root);
}

const round = (n) => Math.round(n * 1000) / 1000;

/** Drops what the passes made unnecessary: the definitions and every id. */
/** The ids something still refers to: by a use, or by a clip that stays. */
function referenced(root) {
  const found = new Set();
  walk(root, (node) => {
    const target = refOf(node);
    if (target) found.add(target);
    const clip = /^url\(#([A-Za-z0-9_-]+)\)$/.exec(attr(node, "clip-path") ?? "");
    if (clip) found.add(clip[1]);
  });
  return found;
}

function strip(root, copyUses) {
  const drop = new Set(["marker", "linearGradient", "radialGradient", "stop"]);
  const used = referenced(root);
  const visit = (node) => {
    node.children = node.children.filter((child) => {
      if (drop.has(child.tag)) return false;
      // Copied, the targets of the uses are in the drawing already.
      if (copyUses && node.tag === "defs" && child.tag !== "clipPath") return false;
      if (child.tag === "clipPath") return used.has(attr(child, "id"));
      return true;
    });
    // A <defs> with nothing left in it goes.
    node.children = node.children.filter((child) => child.tag !== "defs" || child.children.length);
    node.children.forEach(visit);
  };
  visit(root);
  walk(root, (node) => {
    const id = attr(node, "id");
    if (id && !used.has(id)) dropAttr(node, "id");
  });
}

/** Gives the clips that stay ids no other flag or shape has. */
function renameIds(root, prefix) {
  walk(root, (node) => {
    for (const pair of node.attrs) {
      if (pair[0] === "id") pair[1] = `${prefix}${pair[1]}`;
      else if (pair[0] === "xlink:href" || pair[0] === "href") {
        pair[0] = "href";
        pair[1] = `#${prefix}${pair[1].slice(1)}`;
      } else if (pair[0] === "clip-path") {
        const match = /^url\(#([A-Za-z0-9_-]+)\)$/.exec(pair[1]);
        if (match) pair[1] = `url(#${prefix}${match[1]})`;
      }
    }
  });
}

/**
 * Resolves every reference in a parsed flag. The tree is changed in place.
 * Returns how many gradients were flattened. A clip that is not a rectangle stays, with an id
 * that starts with `prefix`.
 */
export function flatten(root, viewBox, where, prefix, copyUses = false) {
  if (copyUses) inlineUses(root, where);
  const gradients = flattenGradients(root, where);
  const kept = new Set();
  inlineClips(root, viewBox, where, kept, copyUses);
  inlineMarkers(root, where);
  strip(root, copyUses);
  renameIds(root, prefix);
  const left = [];
  walk(root, (node) => {
    for (const [name, value] of node.attrs) {
      const own = (name === "clip-path" && /^url\(#[A-Za-z0-9_-]+\)$/.test(value)) || (name === "href" && /^#[A-Za-z0-9_-]+$/.test(value) && node.tag === "use");
      if (!own && (name === "href" || name === "xlink:href" || value.includes("url("))) left.push(`<${node.tag} ${name}=${value}>`);
    }
  });
  if (left.length) throw new Error(`${where}: references are left after flattening: ${left.slice(0, 3).join(" ")}`);
  return gradients;
}

