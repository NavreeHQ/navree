/**
 * The launch film's files (`/media/`) and the manifest that names them.
 *
 * The films and pictures are large and never go into git. They are served
 * from `/home/forge/navree.com/shared/media/` on the live site and, on a
 * developer's machine, from `public/media/`, which `scripts/media.ts` fills
 * from a folder named by `NAVREE_MEDIA_DIR`. What the site may point at is
 * written in `content/media.json`: each file's name, size in bytes and
 * SHA-256. Every place that copies, serves or links these files checks
 * them against that list.
 */
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import type { ReadableStream as WebReadableStream } from "node:stream/web";
import { fileURLToPath } from "node:url";

export const WEBSITE = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
export const MANIFEST_FILE = path.join(WEBSITE, "content", "media.json");
/** Where the dev server and the build find the files: `/media/` in the export. */
export const PUBLIC_MEDIA = path.join(WEBSITE, "public", "media");
/** The URL prefix of the files on the site. */
export const MEDIA_URL = "/media/";

export type MediaFile = { name: string; bytes: number; sha256: string };

/** The files `content/media.json` lists, in its order. */
export function readManifest(): MediaFile[] {
  const parsed = JSON.parse(fs.readFileSync(MANIFEST_FILE, "utf8")) as { files?: MediaFile[] };
  const files = parsed.files ?? [];
  for (const file of files) {
    const ok =
      typeof file.name === "string" &&
      /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(file.name) &&
      Number.isInteger(file.bytes) &&
      file.bytes > 0 &&
      /^[0-9a-f]{64}$/.test(file.sha256);
    if (!ok) throw new Error(`content/media.json: ${JSON.stringify(file)} needs a plain "name", whole "bytes" and a 64-digit lowercase "sha256"`);
  }
  const names = files.map((f) => f.name);
  const twice = names.find((n, i) => names.indexOf(n) !== i);
  if (twice) throw new Error(`content/media.json lists ${twice} twice`);
  return files;
}

/** The SHA-256 of a file, read in 1 MiB pieces. */
export function sha256File(file: string): string {
  const hash = crypto.createHash("sha256");
  const fd = fs.openSync(file, "r");
  try {
    const buffer = Buffer.alloc(1 << 20);
    for (let n = fs.readSync(fd, buffer, 0, buffer.length, null); n > 0; n = fs.readSync(fd, buffer, 0, buffer.length, null)) {
      hash.update(buffer.subarray(0, n));
    }
  } finally {
    fs.closeSync(fd);
  }
  return hash.digest("hex");
}

/** What is wrong with one file next to its manifest entry (null: nothing). */
export function problemWith(file: MediaFile, dir: string): string | null {
  return problemAt(file, path.join(dir, file.name));
}

function problemAt(file: MediaFile, full: string): string | null {
  if (!fs.existsSync(full) || !fs.statSync(full).isFile()) return `${file.name} is missing`;
  const bytes = fs.statSync(full).size;
  if (bytes !== file.bytes) return `${file.name} is ${bytes} bytes, the manifest says ${file.bytes}`;
  const sha256 = sha256File(full);
  if (sha256 !== file.sha256) return `${file.name} has SHA-256 ${sha256}, the manifest says ${file.sha256}`;
  return null;
}

/** Every problem with the files of `dir` against the manifest, and the names in `dir` that the manifest does not list. */
export function checkDir(dir: string): { problems: string[]; unlisted: string[] } {
  const files = readManifest();
  if (!fs.existsSync(dir) || !fs.statSync(dir).isDirectory()) return { problems: [`${dir} is not a folder`], unlisted: [] };
  const problems = files.flatMap((file) => problemWith(file, dir) ?? []);
  const listed = new Set(files.map((f) => f.name));
  const unlisted = fs.readdirSync(dir).filter((name) => !name.startsWith(".") && !listed.has(name));
  return { problems, unlisted };
}

/**
 * Copies the manifest's files from `source` to `public/media/`, after
 * checking each against the manifest, so the dev server and the build serve
 * exactly the listed bytes. A file already in `public/media/` that matches
 * is left alone. Throws, naming every problem, when a source file is
 * missing or differs.
 */
export function syncMedia(source: string): { added: string[]; kept: string[] } {
  const files = readManifest();
  const problems = files.flatMap((file) => problemWith(file, source) ?? []);
  if (problems.length) throw new Error(`${source} does not match content/media.json:\n  ${problems.join("\n  ")}`);
  fs.mkdirSync(PUBLIC_MEDIA, { recursive: true });
  const added: string[] = [];
  const kept: string[] = [];
  for (const file of files) {
    if (problemWith(file, PUBLIC_MEDIA) === null) {
      kept.push(file.name);
      continue;
    }
    fs.copyFileSync(path.join(source, file.name), path.join(PUBLIC_MEDIA, file.name));
    added.push(file.name);
  }
  return { added, kept };
}

/**
 * Downloads the manifest's files from `base` (the live site's `/media/`) to
 * `public/media/`, for a machine that has no copy, such as the daily check on
 * GitHub's runners. Each download is checked against the manifest before it
 * takes its place. Throws when a file cannot be fetched or differs.
 */
export async function fetchMedia(base: string): Promise<{ added: string[]; kept: string[] }> {
  const root = base.endsWith("/") ? base : `${base}/`;
  fs.mkdirSync(PUBLIC_MEDIA, { recursive: true });
  const added: string[] = [];
  const kept: string[] = [];
  for (const file of readManifest()) {
    if (problemWith(file, PUBLIC_MEDIA) === null) {
      kept.push(file.name);
      continue;
    }
    const url = new URL(file.name, root);
    const answer = await fetch(url);
    if (!answer.ok || !answer.body) throw new Error(`${url} answered ${answer.status}`);
    const part = path.join(PUBLIC_MEDIA, `${file.name}.part`);
    await pipeline(Readable.fromWeb(answer.body as WebReadableStream), fs.createWriteStream(part));
    const problem = problemAt(file, part);
    if (problem) {
      fs.rmSync(part, { force: true });
      throw new Error(`${url} does not match content/media.json: ${problem.replace(`${file.name} `, "the download ")}`);
    }
    fs.renameSync(part, path.join(PUBLIC_MEDIA, file.name));
    added.push(file.name);
  }
  return { added, kept };
}
