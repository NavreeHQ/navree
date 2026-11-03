// A strict SVG reader and writer for the artwork the toolkit ships.
//
// Anything it does not know is an error, not something to pass through: a new
// release of an artwork set that adds a script, a style, a link or an element
// the toolkit has never drawn stops the generator, so a person looks first.

/** Elements the shipped artwork may use. Shapes first, then what flags need. */
export const ELEMENTS = new Set([
  "g", "path", "circle", "ellipse", "rect", "line", "polyline", "polygon",
  "use", "defs", "clipPath", "linearGradient", "radialGradient", "stop", "marker",
]);

/** Attributes kept. Presentation and geometry only. */
export const ATTRIBUTES = new Set([
  "d", "cx", "cy", "r", "rx", "ry", "x", "y", "x1", "y1", "x2", "y2", "width", "height",
  "points", "transform", "fill", "stroke", "stroke-width", "stroke-linecap",
  "stroke-linejoin", "stroke-miterlimit", "stroke-dasharray", "stroke-dashoffset",
  "stroke-opacity", "fill-opacity", "fill-rule", "clip-rule", "opacity", "clip-path",
  "marker-start", "marker-mid", "marker-end", "overflow", "offset", "stop-color",
  "stop-opacity", "fx", "fy", "gradientTransform", "gradientUnits", "spreadMethod",
  "markerHeight", "markerWidth", "markerUnits", "refX", "refY", "color", "id", "href",
]);

/** Attributes dropped with no effect on what is drawn: no stylesheet exists, and text is never drawn. */
const DROPPED = new Set([
  "class", "aria-label", "xml:space", "font-family", "font-size", "font-weight",
  "letter-spacing", "text-anchor", "word-spacing", "version", "xmlns", "xmlns:xlink",
]);

/** Style properties that change nothing in a drawing made of shapes. */
const INERT_STYLE = new Set([
  "marker", "line-height", "text-indent", "text-align", "text-decoration-line",
  "text-transform", "-inkscape-stroke", "-inkscape-font-specification",
]);

const TAG = /<([A-Za-z][A-Za-z0-9]*)/y;
const ATTR = /([A-Za-z_:][A-Za-z0-9_:.-]*)\s*=\s*(?:"([^"]*)"|'([^']*)')/y;
const SAFE_VALUE = /^[A-Za-z0-9 .,+\-()#%_]*$/;
const ENTITIES = { quot: '"', amp: "&", lt: "<", gt: ">", apos: "'" };

/** Reads a text into `{tag, attrs: [[name, value]], children}`. */
export function parse(text, where) {
  const fail = (message) => {
    throw new Error(`${where}: ${message}`);
  };
  let at = text.charCodeAt(0) === 0xfeff ? 1 : 0;
  const stack = [];
  let root = null;
  const decode = (raw) =>
    raw.replace(/&(#x[0-9a-fA-F]+|#[0-9]+|[a-z]+);/g, (_, name) => {
      if (name[0] === "#") {
        const code = name[1] === "x" ? parseInt(name.slice(2), 16) : parseInt(name.slice(1), 10);
        return String.fromCodePoint(code);
      }
      if (!(name in ENTITIES)) fail(`unknown entity &${name};`);
      return ENTITIES[name];
    });
  while (at < text.length) {
    if (text[at] !== "<") {
      const end = text.indexOf("<", at);
      const chunk = text.slice(at, end < 0 ? text.length : end);
      if (chunk.trim() !== "") fail(`text outside an element: ${JSON.stringify(chunk.slice(0, 30))}`);
      at = end < 0 ? text.length : end;
      continue;
    }
    if (text.startsWith("<!--", at)) {
      const end = text.indexOf("-->", at);
      if (end < 0) fail("an unclosed comment");
      at = end + 3;
      continue;
    }
    if (text.startsWith("<?xml", at)) {
      const end = text.indexOf("?>", at);
      if (end < 0) fail("an unclosed declaration");
      at = end + 2;
      continue;
    }
    if (text[at + 1] === "!" || text[at + 1] === "?") fail("a DOCTYPE, CDATA or processing instruction");
    if (text[at + 1] === "/") {
      const end = text.indexOf(">", at);
      const name = text.slice(at + 2, end).trim();
      const open = stack.pop();
      if (!open || open.tag !== name) fail(`</${name}> closes nothing`);
      at = end + 1;
      continue;
    }
    // An opening tag.
    TAG.lastIndex = at;
    const match = TAG.exec(text);
    if (!match) fail(`a tag at ${at} cannot be read`);
    const element = { tag: match[1], attrs: [], children: [] };
    at += match[0].length;
    for (;;) {
      while (/\s/.test(text[at] ?? "")) at += 1;
      if (text[at] === "/" && text[at + 1] === ">") {
        at += 2;
        element.selfClosed = true;
        break;
      }
      if (text[at] === ">") {
        at += 1;
        break;
      }
      ATTR.lastIndex = at;
      const attr = ATTR.exec(text);
      if (!attr) fail(`<${element.tag}> has an attribute that cannot be read near ${JSON.stringify(text.slice(at, at + 30))}`);
      element.attrs.push([attr[1], decode(attr[2] ?? attr[3])]);
      at += attr[0].length;
    }
    if (stack.length) stack[stack.length - 1].children.push(element);
    else if (root) fail("more than one root element");
    else root = element;
    if (!element.selfClosed) stack.push(element);
  }
  if (stack.length) fail(`<${stack[stack.length - 1].tag}> is not closed`);
  if (!root) fail("no element");
  return root;
}

/** Escapes an attribute value for a double-quoted attribute. */
export function escapeAttr(value) {
  return value.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;");
}

/** Writes elements with no whitespace between them. */
export function write(element) {
  const attrs = element.attrs.map(([name, value]) => ` ${name}="${escapeAttr(value)}"`).join("");
  if (!element.children.length) return `<${element.tag}${attrs}/>`;
  return `<${element.tag}${attrs}>${element.children.map(write).join("")}</${element.tag}>`;
}

/**
 * The artwork of one flag: a parsed `<svg>` cleaned to what the toolkit draws.
 * Ids are prefixed so that two flags on one page never share one; references
 * to them follow. Returns the inner markup and the root's viewBox.
 */
export function cleanFlag(text, where, idPrefix) {
  const root = parse(text, where);
  if (root.tag !== "svg") throw new Error(`${where}: the root is <${root.tag}>`);
  const viewBox = root.attrs.find(([name]) => name === "viewBox")?.[1];
  if (!viewBox) throw new Error(`${where}: no viewBox`);

  const ids = new Map();
  const collect = (element) => {
    for (const [name, value] of element.attrs) {
      if (name === "id" && element !== root) {
        if (!/^[A-Za-z0-9_-]+$/.test(value)) throw new Error(`${where}: odd id ${JSON.stringify(value)}`);
        if (ids.has(value)) throw new Error(`${where}: id ${value} twice`);
        ids.set(value, `${idPrefix}${value}`);
      }
    }
    element.children.forEach(collect);
  };
  collect(root);

  const wanted = (id) => {
    if (!ids.has(id)) throw new Error(`${where}: a reference to #${id}, which is not defined`);
    return ids.get(id);
  };
  const clean = (element) => {
    if (!ELEMENTS.has(element.tag)) throw new Error(`${where}: <${element.tag}> is not an element the toolkit draws`);
    const attrs = [];
    for (let [name, value] of element.attrs) {
      if (name === "xlink:href") name = "href";
      if (name === "style") {
        for (const declaration of value.split(";").map((d) => d.trim()).filter(Boolean)) {
          const property = declaration.split(":")[0].trim();
          const inert = INERT_STYLE.has(property) || (property === "width" && element.tag === "path");
          if (!inert) throw new Error(`${where}: style property ${property} changes the drawing`);
        }
        continue;
      }
      if (DROPPED.has(name)) continue;
      if (!ATTRIBUTES.has(name)) throw new Error(`${where}: attribute ${name} on <${element.tag}> is not one the toolkit keeps`);
      if (name === "id") {
        attrs.push([name, wanted(value)]);
        continue;
      }
      if (name === "href") {
        if (!/^#[A-Za-z0-9_-]+$/.test(value)) throw new Error(`${where}: href ${JSON.stringify(value)} leaves the drawing`);
        attrs.push([name, `#${wanted(value.slice(1))}`]);
        continue;
      }
      if (!SAFE_VALUE.test(value)) throw new Error(`${where}: ${name}=${JSON.stringify(value.slice(0, 40))} has characters the toolkit does not allow`);
      if (value.includes("url(")) {
        const rewritten = value.replace(/url\(#([A-Za-z0-9_-]+)\)/g, (_, id) => `url(#${wanted(id)})`);
        if (/url\((?!#)/.test(rewritten) || rewritten.replace(/url\(#[A-Za-z0-9_-]+\)/g, "").includes("url(")) {
          throw new Error(`${where}: ${name} refers outside the drawing`);
        }
        attrs.push([name, rewritten]);
        continue;
      }
      attrs.push([name, value]);
    }
    return { tag: element.tag, attrs, children: element.children.map(clean) };
  };

  // The root itself is not shipped: the toolkit draws its own <svg>. Presentation
  // attributes on it (a fill for everything inside) move to a <g> around the children.
  const carried = root.attrs.filter(
    ([name]) => !["viewBox", "id", "width", "height"].includes(name) && !DROPPED.has(name),
  );
  const holder = carried.length ? { tag: "g", attrs: carried, children: root.children } : null;
  const inner = (holder ? [holder] : root.children).map(clean).map(write).join("");
  if (!inner) throw new Error(`${where}: nothing to draw`);
  return { inner, viewBox };
}

/** Rust's raw string literal for `text`. */
export function rustRaw(text) {
  if (text.includes('"##')) throw new Error("a text that closes a raw string");
  return `r##"${text}"##`;
}
