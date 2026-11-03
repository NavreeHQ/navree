// Generates the phone number tables from libphonenumber's numbering plan data,
// `resources/PhoneNumberMetadata.xml` (Apache-2.0), at the release pinned
// below and checked against its SHA-256:
//
//   crates/navree-phone/src/data.rs         the tables the Rust code reads
//   crates/navree-ui/runtime/src/phone-data.json
//                                           what the browser formats with
//
//   node crates/navree-phone/tools/metadata.mjs             # download, check, write
//   node crates/navree-phone/tools/metadata.mjs --check     # exit 1 when an output is stale
//   node crates/navree-phone/tools/metadata.mjs --xml FILE  # use a local copy (still checked)
//
// To move to a newer release, change RELEASE and SHA256 and run it again: the
// Rust code reads the tables through `src/meta.rs` and needs no change.
//
// The patterns are compiled here, once, into a small instruction set over the
// digits 0 to 9 (see `src/regex.rs`), so the Rust side never parses a regular
// expression. The browser gets the patterns as text, for its own RegExp.

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const RELEASE = "v9.0.42";
const SHA256 = "b6491939ddaeb9d585a34fe0f0ebd096e83c5ac9a1c5d6dddf38ba4b162b28ca";
const URL = `https://raw.githubusercontent.com/google/libphonenumber/${RELEASE}/resources/PhoneNumberMetadata.xml`;

const here = dirname(fileURLToPath(import.meta.url));
const crate = join(here, "..");
const RUST_OUT = join(crate, "src", "data.rs");
const JSON_OUT = join(crate, "..", "navree-ui", "runtime", "src", "phone-data.json");

const args = process.argv.slice(2);
const check = args.includes("--check");
const xmlArg = args.includes("--xml") ? args[args.indexOf("--xml") + 1] : null;

// ---------------------------------------------------------------------------
// The source file
// ---------------------------------------------------------------------------

async function source() {
  let bytes;
  if (xmlArg) {
    bytes = readFileSync(xmlArg);
  } else {
    const cache = join(tmpdir(), `navree-phone-${RELEASE}-${SHA256.slice(0, 12)}.xml`);
    if (existsSync(cache)) {
      bytes = readFileSync(cache);
    } else {
      const response = await fetch(URL);
      if (!response.ok) throw new Error(`${URL}: HTTP ${response.status}`);
      bytes = Buffer.from(await response.arrayBuffer());
    }
    if (sha256(bytes) === SHA256 && !existsSync(cache)) writeFileSync(cache, bytes);
  }
  const actual = sha256(bytes);
  if (actual !== SHA256) {
    throw new Error(`PhoneNumberMetadata.xml has SHA-256 ${actual}, not the pinned ${SHA256}`);
  }
  return bytes.toString("utf8");
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// ---------------------------------------------------------------------------
// A reader for the XML: elements, attributes, text and comments. The file is
// pinned by checksum and holds no entities beyond the five predefined ones,
// no CDATA and no processing instructions past the declaration.
// ---------------------------------------------------------------------------

function parseXml(text) {
  const root = { tag: "#root", attrs: {}, children: [], comments: [] };
  const stack = [root];
  let at = 0;
  let pending = [];
  const top = () => stack[stack.length - 1];
  while (at < text.length) {
    if (text.startsWith("<!--", at)) {
      const end = text.indexOf("-->", at);
      if (end < 0) throw new Error("unclosed comment");
      pending.push(text.slice(at + 4, end).trim());
      at = end + 3;
    } else if (text.startsWith("<?", at)) {
      at = text.indexOf("?>", at) + 2;
    } else if (text.startsWith("<!DOCTYPE", at)) {
      // The internal subset declares the elements; nothing in it is needed.
      at = text.indexOf("]>", at) + 2;
    } else if (text.startsWith("</", at)) {
      const end = text.indexOf(">", at);
      const tag = text.slice(at + 2, end).trim();
      const element = stack.pop();
      if (element.tag !== tag) throw new Error(`</${tag}> closes <${element.tag}>`);
      pending = [];
      at = end + 1;
    } else if (text[at] === "<") {
      const end = text.indexOf(">", at);
      let inner = text.slice(at + 1, end);
      const selfClosing = inner.endsWith("/");
      if (selfClosing) inner = inner.slice(0, -1);
      const name = /^[\w:-]+/.exec(inner)[0];
      const attrs = {};
      const attrRe = /([\w:-]+)\s*=\s*"([^"]*)"/g;
      let m;
      while ((m = attrRe.exec(inner.slice(name.length))) !== null) attrs[m[1]] = unescape(m[2]);
      const element = { tag: name, attrs, children: [], text: "", comments: pending };
      pending = [];
      top().children.push(element);
      if (!selfClosing) stack.push(element);
      at = end + 1;
    } else {
      const end = text.indexOf("<", at);
      const chunk = text.slice(at, end < 0 ? text.length : end);
      if (stack.length > 1) top().text += unescape(chunk);
      at = end < 0 ? text.length : end;
    }
  }
  if (stack.length !== 1) throw new Error("unclosed element");
  return root;
}

function unescape(text) {
  return text
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">")
    .replaceAll("&quot;", '"')
    .replaceAll("&apos;", "'")
    .replaceAll("&amp;", "&");
}

const child = (element, tag) => element.children.find((c) => c.tag === tag);
const children = (element, tag) => element.children.filter((c) => c.tag === tag);
const squeeze = (text) => (text ?? "").replace(/\s+/g, "");

// ---------------------------------------------------------------------------
// Patterns: the subset of regular expressions the data uses (digits, `\d`,
// classes of digits, groups, alternation, `?`, `*`, `+`, `{n}`, `{n,m}` and
// `$`), compiled to the instruction set of `src/regex.rs`:
//
//   0x0000 | mask  one digit in the 10-bit mask (bit d is digit d)
//   0x1000 | slot  record the position in capture slot `slot`
//   0x2000         the end of the input
//   0x3000         a match
//   0x8000 | to    jump to `to`
//   0xC000 | to    try the next instruction first, then `to`
//
// Targets count from the pattern's first instruction.
// ---------------------------------------------------------------------------

const ALL_DIGITS = 0x3ff;

function parseRegex(source) {
  let at = 0;
  let groups = 0;
  const fail = (why) => {
    throw new Error(`pattern ${JSON.stringify(source)} at ${at}: ${why}`);
  };
  const peek = () => source[at];
  const eat = (c) => (source[at] === c ? (at++, true) : false);

  function alternation() {
    const items = [sequence()];
    while (eat("|")) items.push(sequence());
    return items.length === 1 ? items[0] : { t: "alt", items };
  }
  function sequence() {
    const items = [];
    while (at < source.length && peek() !== "|" && peek() !== ")") items.push(quantified());
    return { t: "cat", items };
  }
  function quantified() {
    let node = atom();
    for (;;) {
      if (eat("?")) node = { t: "rep", min: 0, max: 1, body: node };
      else if (eat("*")) node = { t: "rep", min: 0, max: Infinity, body: node };
      else if (eat("+")) node = { t: "rep", min: 1, max: Infinity, body: node };
      else if (peek() === "{") {
        const m = /^\{(\d+)(?:(,)(\d*))?\}/.exec(source.slice(at));
        if (!m) fail("bad repetition");
        at += m[0].length;
        const min = Number(m[1]);
        const max = m[2] ? (m[3] === "" ? Infinity : Number(m[3])) : min;
        if (max < min) fail("repetition bounds reversed");
        node = { t: "rep", min, max, body: node };
      } else return node;
    }
  }
  function atom() {
    const c = source[at++];
    if (c === "(") {
      let capture = true;
      if (source.startsWith("?:", at)) {
        at += 2;
        capture = false;
      } else if (source[at] === "?") fail("unsupported group");
      const index = capture ? ++groups : 0;
      const body = alternation();
      if (!eat(")")) fail("unclosed group");
      return capture ? { t: "group", index, body } : body;
    }
    if (c === "[") {
      let mask = 0;
      if (eat("^")) fail("negated class");
      while (peek() !== "]") {
        if (at >= source.length) fail("unclosed class");
        if (source.startsWith("\\d", at)) {
          at += 2;
          mask |= ALL_DIGITS;
          continue;
        }
        const lo = digit(source[at++]);
        if (peek() === "-" && source[at + 1] !== "]") {
          at++;
          const hi = digit(source[at++]);
          if (hi < lo) fail("class range reversed");
          for (let d = lo; d <= hi; d++) mask |= 1 << d;
        } else mask |= 1 << lo;
      }
      at++;
      return { t: "class", mask };
    }
    if (c === "\\") {
      if (source[at++] !== "d") fail("unsupported escape");
      return { t: "class", mask: ALL_DIGITS };
    }
    if (c === "$") return { t: "eol" };
    if (c >= "0" && c <= "9") return { t: "class", mask: 1 << digit(c) };
    fail(`unexpected ${JSON.stringify(c)}`);
  }
  function digit(c) {
    if (!(c >= "0" && c <= "9")) fail(`expected a digit, found ${JSON.stringify(c)}`);
    return c.charCodeAt(0) - 48;
  }

  const node = alternation();
  if (at !== source.length) fail("unbalanced `)`");
  return { node, groups };
}

function compileRegex(source) {
  const { node, groups } = parseRegex(source);
  const code = [];
  const emit = (word) => code.push(word) - 1;
  function gen(n) {
    switch (n.t) {
      case "class":
        if (n.mask === 0) throw new Error(`empty class in ${source}`);
        emit(n.mask);
        break;
      case "eol":
        emit(0x2000);
        break;
      case "cat":
        for (const item of n.items) gen(item);
        break;
      case "group":
        emit(0x1000 | (2 * n.index));
        gen(n.body);
        emit(0x1000 | (2 * n.index + 1));
        break;
      case "alt": {
        const jumps = [];
        n.items.forEach((item, i) => {
          if (i < n.items.length - 1) {
            const split = emit(0);
            gen(item);
            jumps.push(emit(0));
            code[split] = 0xc000 | code.length;
          } else gen(item);
        });
        for (const j of jumps) code[j] = 0x8000 | code.length;
        break;
      }
      case "rep": {
        for (let i = 0; i < n.min; i++) gen(n.body);
        if (n.max === Infinity) {
          const loop = emit(0);
          gen(n.body);
          emit(0x8000 | loop);
          code[loop] = 0xc000 | code.length;
        } else {
          const splits = [];
          for (let i = n.min; i < n.max; i++) {
            splits.push(emit(0));
            gen(n.body);
          }
          for (const s of splits) code[s] = 0xc000 | code.length;
        }
        break;
      }
      default:
        throw new Error(`unknown node ${n.t}`);
    }
  }
  gen(node);
  emit(0x3000);
  if (code.length > 0x3fff) throw new Error(`pattern too long to address: ${source}`);
  if (groups > 7) throw new Error(`too many groups: ${source}`);
  return { code, groups };
}

// ---------------------------------------------------------------------------
// The data, read as libphonenumber's own build reads it.
// ---------------------------------------------------------------------------

/** The number types, in the order of `NumberType`'s descriptions in `src/meta.rs`. */
const TYPES = [
  "fixedLine",
  "mobile",
  "tollFree",
  "premiumRate",
  "sharedCost",
  "personalNumber",
  "voip",
  "pager",
  "uan",
  "voicemail",
  "noInternationalDialling",
];

/** "7,[9-11]" as [7, 9, 10, 11]. */
function lengths(text) {
  const out = new Set();
  if (!text) return out;
  for (const part of text.split(",")) {
    const range = /^\[(\d+)-(\d+)\]$/.exec(part);
    if (range) {
      for (let n = Number(range[1]); n <= Number(range[2]); n++) out.add(n);
    } else if (/^\d+$/.test(part)) out.add(Number(part));
    else throw new Error(`possible lengths ${JSON.stringify(text)}`);
  }
  for (const n of out) if (n < 1 || n > 31) throw new Error(`length ${n} out of range`);
  return out;
}

function mask(set) {
  let out = 0;
  for (const n of set) out |= 2 ** n;
  return out >>> 0;
}

function readTerritory(element) {
  const a = element.attrs;
  const id = a.id;
  const nationalPrefix = a.nationalPrefix ?? "";
  // The English name, from the comment above the territory: "Name (ID)".
  let name = "";
  for (const comment of element.comments) {
    const m = /^(.+?)\s*\(([A-Z]{2}|001)\)$/.exec(comment.split("\n")[0].trim());
    if (m && m[2] === id) name = m[1];
  }
  if (!name && id !== "001") throw new Error(`no name for ${id}`);

  const general = child(element, "generalDesc");
  const descs = {};
  const allLengths = new Set();
  const allLocal = new Set();
  for (const type of TYPES) {
    const desc = child(element, type);
    if (!desc) continue;
    const possible = child(desc, "possibleLengths");
    const national = lengths(possible?.attrs.national);
    const local = lengths(possible?.attrs.localOnly);
    descs[type] = {
      pattern: squeeze(child(desc, "nationalNumberPattern")?.text),
      lengths: national,
      local,
      example: squeeze(child(desc, "exampleNumber")?.text),
    };
    // noInternationalDialling does not count towards the general lengths.
    if (type !== "noInternationalDialling") {
      for (const n of national) allLengths.add(n);
      for (const n of local) allLocal.add(n);
    }
  }
  for (const n of allLengths) allLocal.delete(n);
  const generalDesc = {
    pattern: squeeze(child(general, "nationalNumberPattern")?.text),
    lengths: allLengths,
    local: allLocal,
    example: squeeze(child(general, "exampleNumber")?.text),
  };
  // A type with the same lengths as the whole territory states them anyway here.
  for (const type of TYPES) {
    if (descs[type] && descs[type].lengths.size === 0) descs[type].lengths = new Set(allLengths);
  }

  const formats = [];
  const intl = [];
  let explicitIntl = false;
  const available = child(element, "availableFormats");
  for (const f of available ? children(available, "numberFormat") : []) {
    const rule = f.attrs.nationalPrefixFormattingRule ?? a.nationalPrefixFormattingRule ?? "";
    const optional =
      (f.attrs.nationalPrefixOptionalWhenFormatting ?? a.nationalPrefixOptionalWhenFormatting) === "true";
    const format = {
      pattern: squeeze(f.attrs.pattern),
      format: child(f, "format").text.trim(),
      leading: children(f, "leadingDigits").map((l) => squeeze(l.text)),
      rule: rule.replace("$NP", nationalPrefix).replace("$FG", "$1"),
      optional,
      carrier: f.attrs.carrierCodeFormattingRule ?? a.carrierCodeFormattingRule ?? "",
    };
    formats.push(format);
    const intlElement = child(f, "intlFormat");
    if (intlElement) {
      explicitIntl = true;
      const text = intlElement.text.trim();
      if (text !== "NA") intl.push({ ...format, format: text });
    } else intl.push(format);
  }

  return {
    id,
    name,
    code: Number(a.countryCode),
    main: a.mainCountryForCode === "true",
    idd: squeeze(a.internationalPrefix),
    preferredIdd: a.preferredInternationalPrefix ?? "",
    nationalPrefix,
    // Without its own, a territory strips its national prefix.
    npp: squeeze(a.nationalPrefixForParsing) || nationalPrefix,
    npt: a.nationalPrefixTransformRule ?? "",
    extn: a.preferredExtnPrefix ?? "",
    leading: squeeze(a.leadingDigits),
    portable: a.mobileNumberPortableRegion === "true",
    general: generalDesc,
    descs,
    formats,
    intl: explicitIntl ? intl : [],
  };
}

/** The national format of a pattern: the first group written with the national prefix rule. */
function nationalFormat(format) {
  if (!format.rule) return "";
  const m = /\$\d/.exec(format.format);
  if (!m) return "";
  const out = format.format.slice(0, m.index) + format.rule.replace("$1", m[0]) + format.format.slice(m.index + 2);
  return out === format.format ? "" : out;
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

async function main() {
  const xml = parseXml(await source());
  const metadata = child(xml, "phoneNumberMetadata");
  const territories = children(child(metadata, "territories"), "territory").map(readTerritory);

  // Patterns are stored once each, back to back.
  const code = [];
  const patterns = new Map();
  const pat = (source) => {
    if (!source) return "NONE";
    if (!patterns.has(source)) {
      const compiled = compileRegex(source);
      patterns.set(source, `p(${code.length}, ${compiled.code.length}, ${compiled.groups})`);
      code.push(...compiled.code);
    }
    return patterns.get(source);
  };
  // So is text, at byte offsets of its UTF-8 form.
  let text = "";
  const texts = new Map();
  const bytes = (value) => Buffer.byteLength(value, "utf8");
  const txt = (value) => {
    if (!value) return "EMPTY";
    if (!texts.has(value)) {
      const at = text.indexOf(value);
      if (at >= 0) texts.set(value, `t(${bytes(text.slice(0, at))}, ${bytes(value)})`);
      else {
        texts.set(value, `t(${bytes(text)}, ${bytes(value)})`);
        text += value;
      }
    }
    return texts.get(value);
  };
  for (const t of territories) {
    for (const value of [t.idd, t.npp, t.leading, t.general.pattern]) if (value) pat(value);
  }

  const descs = [];
  const descLine = (d) => {
    for (const n of d.lengths) if (d.local.has(n)) d.local.delete(n);
    return `d(${pat(d.pattern)}, 0x${mask(d.lengths).toString(16)}, 0x${mask(d.local).toString(16)}, ${txt(d.example)})`;
  };
  const formats = [];
  const formatLine = (f) =>
    `f(${pat(f.pattern)}, ${pat(f.leading.at(-1) ?? "")}, ${txt(f.format)}, ${txt(nationalFormat(f))})`;

  // Geographic territories by id, then the non-geographic ones by calling code.
  const geo = territories.filter((t) => t.id !== "001").sort((a, b) => a.id.localeCompare(b.id));
  const nonGeo = territories.filter((t) => t.id === "001").sort((a, b) => a.code - b.code);
  const ordered = [...geo, ...nonGeo];
  for (const [i, t] of geo.entries()) if (i > 0 && geo[i - 1].id === t.id) throw new Error(`two ${t.id}`);

  const lines = [];
  for (const t of ordered) {
    const typeDescs = TYPES.map((type) => {
      const d = t.descs[type];
      if (!d || !d.pattern) return 0;
      descs.push(descLine(d));
      return descs.length;
    });
    const formatStart = formats.length;
    for (const f of t.formats) formats.push(formatLine(f));
    const intlStart = formats.length;
    for (const f of t.intl) formats.push(formatLine(f));
    const fixed = t.descs.fixedLine?.pattern;
    const same = Boolean(fixed) && fixed === t.descs.mobile?.pattern;
    lines.push(
      [
        "    Territory {",
        `        id: *b"${t.id === "001" ? "00" : t.id}",`,
        `        code: ${t.code},`,
        `        name: ${txt(t.name)},`,
        `        idd: ${pat(t.idd)},`,
        `        national_prefix: ${txt(t.nationalPrefix)},`,
        `        npp: ${pat(t.npp)},`,
        `        npt: ${txt(t.npt)},`,
        `        extension_prefix: ${txt(t.extn)},`,
        `        leading: ${pat(t.leading)},`,
        `        general: ${descLine(t.general)},`,
        `        types: [${typeDescs.join(", ")}],`,
        `        formats: (${formatStart}, ${t.formats.length}),`,
        `        intl_formats: (${intlStart}, ${t.intl.length}),`,
        `        same_mobile_and_fixed: ${same},`,
        "    },",
      ].join("\n"),
    );
  }

  // Calling codes, each with its territories: the main one first, then the
  // rest in the file's order (which is by id).
  const byCode = new Map();
  for (const t of territories) {
    const list = byCode.get(t.code) ?? [];
    if (t.main) list.unshift(t);
    else list.push(t);
    byCode.set(t.code, list);
  }
  const codeEntries = [...byCode.entries()].sort((a, b) => a[0] - b[0]);
  const codeTerritories = [];
  const codeLines = codeEntries.map(([cc, list]) => {
    const start = codeTerritories.length;
    for (const t of list) codeTerritories.push(ordered.indexOf(t));
    return `(${cc}, ${start}, ${list.length})`;
  });

  const wrap = (items, per) => {
    const out = [];
    for (let i = 0; i < items.length; i += per) out.push(`    ${items.slice(i, i + per).join(", ")},`);
    return out.join("\n");
  };

  let rust = `//! The numbering plans: generated by \`tools/metadata.mjs\` from
//! libphonenumber's \`resources/PhoneNumberMetadata.xml\`, release ${RELEASE}
//! (SHA-256 \`${SHA256}\`), under the Apache License 2.0
//! (see THIRD_PARTY.md). Do not edit by hand: run
//! \`node crates/navree-phone/tools/metadata.mjs\`.

use crate::meta::{Desc, EMPTY, Format, NONE, Territory, d, f, p, t};

/// The release of the numbering plans.
pub(crate) const RELEASE: &str = "${RELEASE}";

/// Every compiled pattern, back to back (see \`regex.rs\`).
#[rustfmt::skip]
pub(crate) static CODE: [u16; ${code.length}] = [
${wrap(code.map((w) => `0x${w.toString(16).padStart(4, "0")}`), 12)}
];

/// Every name, format, prefix and example number, back to back.
pub(crate) static TEXT: &str = ${JSON.stringify(text)};

/// The descriptions of number types; \`Territory::types\` holds an index
/// plus one into it, 0 for a type the territory has none of.
#[rustfmt::skip]
pub(crate) static DESCS: [Desc; ${descs.length}] = [
${descs.map((d) => `    ${d},`).join("\n")}
];

/// Number formats, a territory's national ones then its international ones.
#[rustfmt::skip]
pub(crate) static FORMATS: [Format; ${formats.length}] = [
${formats.map((f) => `    ${f},`).join("\n")}
];

/// The territories: the geographic ones by id, then the non-geographic
/// ones (id \`001\`) by calling code.
#[rustfmt::skip]
pub(crate) static TERRITORIES: [Territory; ${ordered.length}] = [
${lines.join("\n")}
];

/// The number of geographic territories at the start of \`TERRITORIES\`.
pub(crate) const GEOGRAPHIC: usize = ${geo.length};

/// Calling codes, ascending: the code, then the range of
/// \`CODE_TERRITORIES\` holding its territories, the main one first.
#[rustfmt::skip]
pub(crate) static CODES: [(u16, u16, u16); ${codeLines.length}] = [
${wrap(codeLines, 6)}
];

/// Indexes into \`TERRITORIES\`, grouped by calling code.
#[rustfmt::skip]
pub(crate) static CODE_TERRITORIES: [u16; ${codeTerritories.length}] = [
${wrap(codeTerritories, 16)}
];
`;

  // The browser: what it needs to format as you type, detect a country from
  // a pasted international number and write E.164.
  const browserTerritory = (t) => {
    const main = byCode.get(t.code)[0];
    const entry = {
      id: t.id,
      cc: t.code,
      idd: t.idd,
      np: t.nationalPrefix,
      npp: t.npp,
      npt: t.npt,
      ld: t.leading,
      gd: t.general.pattern,
      len: [...t.general.lengths].sort((a, b) => a - b),
      ex: t.descs.mobile?.example || t.descs.fixedLine?.example || "",
    };
    if (main === t) {
      entry.f = t.formats.map((f) => [f.pattern, f.format, f.leading, nationalFormat(f) || 0, f.optional ? 1 : 0]);
      if (t.intl.length) entry.i = t.intl.map((f) => [f.pattern, f.format, f.leading]);
    }
    if (byCode.get(t.code).length > 1 && !t.leading) {
      // Territories that share a code and have no leading digits of their
      // own (the United States and Canada; Australia and its islands) are
      // told apart by their lines, tried in the server's order, main first.
      const lines = ["fixedLine", "mobile"].map((type) => t.descs[type]?.pattern).filter(Boolean);
      entry.tp = lines.map((p) => `(?:${p})`).join("|");
    }
    for (const key of Object.keys(entry)) if (entry[key] === "") delete entry[key];
    return entry;
  };
  const json = `${JSON.stringify({ v: RELEASE, t: geo.map(browserTerritory) })}\n`;

  const outputs = [
    [RUST_OUT, rust],
    [JSON_OUT, json],
  ];
  if (check) {
    const stale = outputs.filter(([path, body]) => !existsSync(path) || readFileSync(path, "utf8") !== body);
    for (const [path] of stale) console.error(`stale: ${path}`);
    if (stale.length) {
      console.error("run: node crates/navree-phone/tools/metadata.mjs");
      process.exit(1);
    }
    console.log("up to date");
  } else {
    for (const [path, body] of outputs) {
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, body);
    }
  }
  console.log(
    `${RELEASE}: ${geo.length} regions, ${nonGeo.length} non-geographic, ${codeEntries.length} calling codes, ` +
      `${patterns.size} patterns in ${code.length} instructions, ${formats.length} formats, ${descs.length} type descriptions, ` +
      `${text.length} bytes of text; browser data ${json.length} bytes`,
  );
}

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
