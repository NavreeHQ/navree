// Fetches a pinned npm tarball, refuses it unless its SHA-512 matches the pin,
// and reads the files out of it. No dependencies: the tarball is gunzipped
// with node:zlib and read with a small tar reader.

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { gunzipSync } from "node:zlib";

/** Where downloads are kept between runs. Every use checks the pin again. */
export function cacheDir() {
  const dir = process.env.NAVREE_ICON_CACHE || join(tmpdir(), "navree-ui-icon-cache");
  mkdirSync(dir, { recursive: true });
  return dir;
}

/** `sha512-<base64>`, the form npm records as a tarball's integrity. */
export function integrityOf(bytes) {
  return `sha512-${createHash("sha512").update(bytes).digest("base64")}`;
}

/** The SHA-256 of a text, in hex. */
export function sha256(text) {
  return createHash("sha256").update(text).digest("hex");
}

/**
 * The tarball of one pinned source, as bytes that have matched the pin.
 * @param {{package: string, version: string, url: string, integrity: string}} pin
 */
export async function tarball(pin) {
  const file = join(cacheDir(), `${pin.package}-${pin.version}.tgz`);
  if (existsSync(file)) {
    const bytes = readFileSync(file);
    if (integrityOf(bytes) === pin.integrity) return bytes;
    rmSync(file); // a damaged or replaced cache entry: fetch again
  }
  const response = await fetch(pin.url);
  if (!response.ok) throw new Error(`${pin.url}: HTTP ${response.status}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  const got = integrityOf(bytes);
  if (got !== pin.integrity) {
    throw new Error(
      `${pin.package}@${pin.version}: the tarball's checksum is ${got}, the pin says ${pin.integrity}. Nothing was read from it.`,
    );
  }
  writeFileSync(file, bytes);
  return bytes;
}

/** `{ "package/icons/x.svg": Buffer }` for every regular file in a .tgz. */
export function untar(tgz) {
  const data = gunzipSync(tgz);
  const files = new Map();
  let pending = {};
  let at = 0;
  const text = (from, length) => {
    const raw = data.subarray(at + from, at + from + length);
    const end = raw.indexOf(0);
    return raw.subarray(0, end < 0 ? raw.length : end).toString("utf8");
  };
  while (at + 512 <= data.length) {
    if (data.subarray(at, at + 512).every((byte) => byte === 0)) break;
    const size = parseInt(text(124, 12).trim() || "0", 8);
    const type = text(156, 1) || "0";
    const prefix = text(345, 155);
    let name = (prefix ? `${prefix}/` : "") + text(0, 100);
    const body = data.subarray(at + 512, at + 512 + size);
    if (type === "x") {
      // pax header: "<length> key=value\n" records
      for (const line of body.toString("utf8").split("\n")) {
        const match = line.match(/^\d+ ([^=]+)=(.*)$/);
        if (match) pending[match[1]] = match[2];
      }
    } else if (type === "L") {
      pending.path = body.toString("utf8").replace(/\0.*$/s, "");
    } else if (type === "0") {
      name = pending.path ?? name;
      if (name.startsWith("/") || name.split("/").includes("..")) {
        throw new Error(`the tarball holds an unsafe path: ${name}`);
      }
      files.set(name, Buffer.from(body));
      pending = {};
    } else if (type !== "g") {
      pending = {};
    }
    at += 512 + Math.ceil(size / 512) * 512;
  }
  return files;
}

/** The files of a pinned npm package, by path inside `package/`. */
export async function packageFiles(pin) {
  const files = untar(await tarball(pin));
  const out = new Map();
  for (const [name, bytes] of files) {
    if (name.startsWith("package/")) out.set(name.slice("package/".length), bytes);
  }
  return out;
}

/** The registry's record of one version: its tarball URL and integrity. */
export async function registryPin(packageName, version) {
  const response = await fetch(`https://registry.npmjs.org/${packageName}/${version}`);
  if (!response.ok) throw new Error(`${packageName}@${version}: HTTP ${response.status}`);
  const meta = await response.json();
  return {
    package: packageName,
    version: meta.version,
    url: meta.dist.tarball,
    integrity: meta.dist.integrity,
    licence: meta.license,
  };
}
