// Draws the site's illustrations with Gemini image generation and saves them
// as WebP under public/images/generated/, with a manifest of alt texts.
//
//   node scripts/generate-images.ts                 every missing image, one Batch API job
//   node scripts/generate-images.ts --direct        the same, one generateContent call each (faster)
//   node scripts/generate-images.ts --force [key…]  redraw (all, or the named keys)
//   node scripts/generate-images.ts --poll          collect a batch started earlier
//   node scripts/generate-images.ts --manifest      rebuild the manifest only
//
// The key comes from GEMINI_API_KEY or the git-ignored file `gemini.token` at
// the repository root. It is sent only in the x-goog-api-key header and never
// printed. The model is GEMINI_IMAGE_MODEL (default gemini-3.1-flash-lite-image).
//
// Look at every image before using it: redraw anything with lettering,
// digits, logos, UI, or a busy composition.
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";

const root = path.resolve(import.meta.dirname, "..");
const repo = path.resolve(root, "..", "..");
const outDir = path.join(root, "public", "images", "generated");
const stateFile = path.join(repo, "target", "image-batches", "website.json");
// The full-size PNGs as the model returned them (git-ignored, not shipped).
const rawDir = path.join(repo, "target", "image-raw");

// sharp ships with Next; resolve it from there rather than adding a dependency.
// Typed here with the few calls this script makes, since sharp is not a direct dependency.
interface SharpImage {
  resize(options: { width: number; withoutEnlargement: boolean }): SharpImage;
  webp(options: { quality: number; effort: number }): SharpImage;
  toFile(file: string): Promise<unknown>;
  metadata(): Promise<{ width?: number; height?: number }>;
}
const sharp = createRequire(createRequire(import.meta.url).resolve("next/package.json"))("sharp") as (input: Buffer | string) => SharpImage;

const MODEL = process.env.GEMINI_IMAGE_MODEL ?? "gemini-3.1-flash-image";
// "1K", "2K" or "4K": the size the model renders at, before the WebP resize.
const IMAGE_SIZE = process.env.GEMINI_IMAGE_SIZE ?? "4K";
const API = "https://generativelanguage.googleapis.com/v1beta";
const args = process.argv.slice(2);
const FORCE = args.includes("--force");
const DIRECT = args.includes("--direct");
const POLL = args.includes("--poll");
const MANIFEST_ONLY = args.includes("--manifest");
const ONLY = args.filter((a) => !a.startsWith("--"));

function apiKey(): string {
  if (process.env.GEMINI_API_KEY) return process.env.GEMINI_API_KEY.trim();
  const file = path.join(repo, "gemini.token");
  if (fs.existsSync(file)) return fs.readFileSync(file, "utf8").trim();
  throw new Error("No GEMINI_API_KEY and no gemini.token at the repository root");
}

const redact = (s: string) => s.replace(/AQ\.[A-Za-z0-9_-]+|AIza[A-Za-z0-9_-]+/g, "[redacted]");

const STYLE = `
Art direction (identical for every image in this set; they sit together on one website and must look like one series by one studio):
- A premium, minimal, cinematic 3D render, like a high-end product photograph in a dark studio.
- Materials: matte black ceramic, black anodized aluminium, smoked glass and polished black obsidian. No other colors except the light.
- Light: one single neon green light, exactly #3DFF8F, used as the accent (a glow, a beam, a thin edge light, a reflection). Everything else is lit by very faint cool rim light.
- Background: pure, deep black (#030504) that fades seamlessly to black at every edge, so the image blends into a black web page. Generous negative space.
- Calm, precise, engineered, timeless. Shallow depth of field, crisp details, subtle film grain.
Hard rules:
- NO text, letters, numbers, digits, labels, logos, brand marks, watermarks or user-interface elements anywhere, not even blurred or made-up ones.
- NO people, no hands, no faces.
- NO purple, blue, pink or orange. NO rainbow gradients. NO lens flares.
- One image, full-bleed to every edge: no border, frame or split layout.`.trim();

type Brief = { key: string; ratio: "1:1" | "16:9" | "21:9" | "4:3"; width: number; alt: string; subject: string };

const BRIEFS: Brief[] = [
  // The tools: one object each, centered, for product cards and feature blocks.
  {
    key: "tool-lighthouse",
    ratio: "1:1",
    width: 2048,
    alt: "A minimal black lighthouse casting a green beam across the dark",
    subject:
      "A sleek, minimal lighthouse tower sculpted from matte black ceramic, standing on a small black rock. Its lamp room is smoked glass and emits one sharp, thin neon green beam that cuts diagonally across the darkness. Centered, seen from a low three-quarter angle.",
  },
  {
    key: "tool-fleet",
    ratio: "1:1",
    width: 2048,
    alt: "Five small black vessels in precise formation with green running lights",
    subject:
      "Five small, minimal, abstract vessels of matte black metal moving in a precise V formation across a perfectly still black reflective surface, seen from slightly above. Each vessel has one thin neon green running light along its deck line, reflected in the water.",
  },
  {
    key: "tool-harbor",
    ratio: "1:1",
    width: 2048,
    alt: "A black gantry crane lowering a glowing green cube onto a pier",
    subject:
      "A minimal harbor gantry crane of black anodized aluminium lowering a single glowing neon green glass cube onto the end of a black stone pier. Precise industrial design, centered, three-quarter view.",
  },
  {
    key: "tool-helm",
    ratio: "1:1",
    width: 2048,
    alt: "A precision-machined black ship's wheel with a thin green light ring",
    subject:
      "A ship's helm wheel reimagined as a precision-machined object: black anodized aluminium hub and eight spokes, with a thin neon green light ring set into the rim. Floating slightly tilted toward the viewer.",
  },
  {
    key: "tool-logbook",
    ratio: "1:1",
    width: 2048,
    alt: "A closed black leather logbook with a glowing green ribbon",
    subject:
      "A closed, hardbound logbook in black leather lying at an angle on a black obsidian surface, with a glowing neon green silk ribbon bookmark trailing out of its pages. The cover is completely blank.",
  },
  {
    key: "tool-dock",
    ratio: "1:1",
    width: 2048,
    alt: "A neat grid of black containers on a dock, one lit green from within",
    subject:
      "A neat three-by-three grid of small matte black shipping-container shapes on a black dock, seen from an isometric angle above. One container in the grid glows softly neon green from within. The container sides are plain, with no markings.",
  },
  {
    key: "tool-navigator",
    ratio: "1:1",
    width: 2048,
    alt: "A black titanium compass with a glowing green needle",
    subject:
      "A precision navigation compass of black titanium and smoked glass resting on a black obsidian surface, its slim needle glowing neon green. The dial is perfectly plain, with no marks, numbers or letters.",
  },
  // Page heroes: wide, quiet, room for text on the left.
  {
    key: "hero-hull",
    ratio: "21:9",
    width: 3200,
    alt: "The bow of a sleek black hull in still dark water, its edge traced by green light",
    subject:
      "The bow of a sleek, minimal, modern black yacht hull seen in profile at water level, emerging from darkness on the right side of the frame. One continuous thin neon green light line traces the hull's upper edge from the bow backward and is mirrored in the perfectly still black water. Only the hull and its reflection; no masts, no railings, no structures, nothing below the waterline shown. The left half of the frame is calm empty darkness.",
  },
  {
    key: "hero-archipelago",
    ratio: "21:9",
    width: 3200,
    alt: "Black modular islands linked by thin bridges of green light",
    subject:
      "Seen from high above at night: an archipelago of small, precise, modular black islands of different sizes on black water, connected by thin straight bridges of neon green light, forming a clear network. The network sits in the right two-thirds of the frame; the left side is empty darkness.",
  },
  {
    key: "hero-constellation",
    ratio: "21:9",
    width: 3200,
    alt: "Green star points joined into a navigation route across a black sky",
    subject:
      "A black night sky with a few dozen tiny neon green star points joined by hair-thin green lines into a calm navigation route that curves from the lower left to the upper right, like a ship's course plotted across the stars. Very sparse and elegant. Only the stars and the lines on pure black: no planets, no horizon, no curves of any object, no ships, no text, no symbols.",
  },
  {
    key: "hero-wake",
    ratio: "21:9",
    width: 3200,
    alt: "A black hull cutting through still water, leaving a wake of green light",
    subject:
      "A long exposure of a sleek black hull moving fast across perfectly still black water, seen from above at an angle, leaving a crisp V-shaped wake of neon green light behind it. The hull is in the right third of the frame; the wake trails toward the left.",
  },
  {
    key: "hero-rooms",
    ratio: "21:9",
    width: 3200,
    alt: "A row of black glass rooms, each softly lit, one glowing green",
    subject:
      "An isometric row of identical small rooms made of black smoked glass, separated by thin walls, like cells in a precise structure. Each room is softly lit from inside with faint cool light; exactly one room glows neon green. The row recedes from the right side toward the center; the left side is empty darkness.",
  },
  {
    key: "hero-harbor",
    ratio: "21:9",
    width: 3200,
    alt: "A minimal harbor at night with vessels in their berths and a green lighthouse beam",
    subject:
      "A minimal harbor at night seen from high above: piers drawn as fine black lines on black water, vessels of different sizes resting in orderly berths, and one small lighthouse on the breakwater sweeping a thin neon green beam across the water. Composition weighted to the right; the left side calm and empty.",
  },
];

function request(brief: Brief) {
  return {
    contents: [{ role: "user", parts: [{ text: `${brief.subject}\n\n${STYLE}` }] }],
    generationConfig: { responseModalities: ["IMAGE"], imageConfig: { aspectRatio: brief.ratio, imageSize: IMAGE_SIZE } },
  };
}

type InlineData = { data?: string };
type Part = { inlineData?: InlineData; inline_data?: InlineData };
type GenerateResponse = { candidates?: { content?: { parts?: Part[] } }[] };
type InlinedResponse = { metadata?: { key?: string }; error?: unknown; response?: GenerateResponse };
type Operation = {
  name?: string;
  state?: string;
  done?: boolean;
  error?: unknown;
  metadata?: { state?: string };
  response?: {
    inlinedResponses?: { inlinedResponses?: InlinedResponse[] };
    inlined_responses?: { inlined_responses?: InlinedResponse[] };
  };
};

async function call<T>(url: string, init: RequestInit = {}): Promise<T> {
  const res = await fetch(url, {
    ...init,
    headers: { "Content-Type": "application/json", "x-goog-api-key": apiKey(), ...(init.headers ?? {}) },
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`${res.status} ${redact(text).slice(0, 600)}`);
  return JSON.parse(text) as T;
}

function imageFrom(response: GenerateResponse | undefined): Buffer | null {
  const parts = response?.candidates?.[0]?.content?.parts ?? [];
  const data = parts.find((p) => p.inlineData?.data ?? p.inline_data?.data);
  const b64 = data?.inlineData?.data ?? data?.inline_data?.data;
  return b64 ? Buffer.from(b64, "base64") : null;
}

async function save(brief: Brief, image: Buffer) {
  fs.mkdirSync(outDir, { recursive: true });
  fs.mkdirSync(rawDir, { recursive: true });
  fs.writeFileSync(path.join(rawDir, `${brief.key}.png`), image);
  const file = path.join(outDir, `${brief.key}.webp`);
  await sharp(image).resize({ width: brief.width, withoutEnlargement: true }).webp({ quality: 86, effort: 6 }).toFile(file);
  console.log(`saved ${path.relative(root, file)} (${Math.round(fs.statSync(file).size / 1024)} KB)`);
}

async function writeManifest() {
  const entries = [];
  for (const brief of BRIEFS) {
    const file = path.join(outDir, `${brief.key}.webp`);
    if (!fs.existsSync(file)) continue;
    const meta = await sharp(file).metadata();
    entries.push({ key: brief.key, src: `/images/generated/${brief.key}.webp`, width: meta.width, height: meta.height, alt: brief.alt });
  }
  fs.mkdirSync(outDir, { recursive: true });
  fs.writeFileSync(path.join(outDir, "manifest.json"), `${JSON.stringify({ model: MODEL, images: entries }, null, 2)}\n`);
  console.log(`manifest: ${entries.length} image(s)`);
}

function todo(): Brief[] {
  return BRIEFS.filter((b) => {
    if (ONLY.length) return ONLY.includes(b.key);
    return FORCE || !fs.existsSync(path.join(outDir, `${b.key}.webp`));
  });
}

async function direct(briefs: Brief[]) {
  await Promise.all(
    briefs.map(async (brief) => {
      try {
        const res = await call<GenerateResponse>(`${API}/models/${MODEL}:generateContent`, { method: "POST", body: JSON.stringify(request(brief)) });
        const image = imageFrom(res);
        if (!image) throw new Error(`no image in the response: ${redact(JSON.stringify(res)).slice(0, 300)}`);
        await save(brief, image);
      } catch (error) {
        console.error(`${brief.key}: ${redact(String(error))}`);
      }
    }),
  );
}

async function startBatch(briefs: Brief[]) {
  const body = {
    batch: {
      display_name: `navree-website-${briefs.length}`,
      input_config: { requests: { requests: briefs.map((b) => ({ request: request(b), metadata: { key: b.key } })) } },
    },
  };
  const op = await call<Operation>(`${API}/models/${MODEL}:batchGenerateContent`, { method: "POST", body: JSON.stringify(body) });
  fs.mkdirSync(path.dirname(stateFile), { recursive: true });
  fs.writeFileSync(stateFile, JSON.stringify({ name: op.name, keys: briefs.map((b) => b.key) }));
  console.log(`batch started: ${op.name} (${briefs.length} request(s)); collect with --poll`);
}

async function poll() {
  const state = JSON.parse(fs.readFileSync(stateFile, "utf8"));
  const op = await call<Operation>(`${API}/${state.name}`);
  const status = op.metadata?.state ?? op.state ?? (op.done ? "done" : "unknown");
  console.log(`batch ${state.name}: ${status}`);
  if (!op.done) return false;
  if (op.error) throw new Error(redact(JSON.stringify(op.error)));
  const inlined = op.response?.inlinedResponses?.inlinedResponses ?? op.response?.inlined_responses?.inlined_responses ?? [];
  for (const [i, item] of inlined.entries()) {
    const key = item.metadata?.key ?? state.keys[i];
    const brief = BRIEFS.find((b) => b.key === key);
    if (!brief) continue;
    if (item.error) {
      console.error(`${key}: ${redact(JSON.stringify(item.error))}`);
      continue;
    }
    const image = imageFrom(item.response);
    if (image) await save(brief, image);
    else console.error(`${key}: no image in the response`);
  }
  return true;
}

if (MANIFEST_ONLY) {
  await writeManifest();
} else if (POLL) {
  if (await poll()) await writeManifest();
} else {
  const briefs = todo();
  if (!briefs.length) console.log("nothing to draw");
  else if (DIRECT) await direct(briefs);
  else await startBatch(briefs);
  if (DIRECT) await writeManifest();
}
