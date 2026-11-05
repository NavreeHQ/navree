/**
 * The launch film's files, which never go into git (`scripts/lib/media.ts`
 * says why and where they live).
 *
 *   NAVREE_MEDIA_DIR=<folder> node scripts/media.ts sync
 *       Copies the files `content/media.json` lists from the folder to
 *       `public/media/`, for the dev server and the build. `pnpm dev`,
 *       `pnpm build` and `pnpm typecheck` do this on their own when
 *       NAVREE_MEDIA_DIR is set (`scripts/prebuild.ts`).
 *   NAVREE_MEDIA_URL=https://navree.com/media/ node scripts/media.ts fetch
 *       Downloads the same files from the live site, checking each, for a
 *       machine with no copy (the daily check on GitHub's runners). The
 *       build does this on its own when NAVREE_MEDIA_URL is set and
 *       NAVREE_MEDIA_DIR is not.
 *   node scripts/media.ts verify <folder>
 *       Checks every listed file in the folder (the live server's
 *       `shared/media/`, which `deploy/navree.com/deploy.sh` runs this on)
 *       against its name, size and SHA-256. Exits 1 on any difference.
 *   node scripts/media.ts manifest <folder>
 *       Writes `content/media.json` from the files in the folder, for when a
 *       film or picture changes. Review the diff, commit it, then copy the
 *       new files to the server.
 */
import fs from "node:fs";
import path from "node:path";

import { MANIFEST_FILE, checkDir, fetchMedia, readManifest, sha256File, syncMedia, type MediaFile } from "./lib/media.ts";

const [command, argument] = process.argv.slice(2);

function fail(message: string): never {
  console.error(`media: ${message}`);
  process.exit(1);
}

const megabytes = (bytes: number) => `${(bytes / 1_000_000).toFixed(1)} MB`;

if (command === "sync") {
  const source = argument ?? process.env.NAVREE_MEDIA_DIR;
  if (!source) fail("set NAVREE_MEDIA_DIR to the folder that holds the launch film's files, or pass the folder");
  try {
    const { added, kept } = syncMedia(path.resolve(source));
    console.log(`media: public/media/ has the ${added.length + kept.length} files of content/media.json (${added.length} copied, ${kept.length} already there)`);
  } catch (error) {
    fail((error as Error).message);
  }
} else if (command === "fetch") {
  const base = argument ?? process.env.NAVREE_MEDIA_URL;
  if (!base) fail("set NAVREE_MEDIA_URL to the address of the live site's /media/, or pass it");
  try {
    const { added, kept } = await fetchMedia(base);
    console.log(`media: public/media/ has the ${added.length + kept.length} files of content/media.json (${added.length} downloaded, ${kept.length} already there)`);
  } catch (error) {
    fail((error as Error).message);
  }
} else if (command === "verify") {
  if (!argument) fail("usage: node scripts/media.ts verify <folder>");
  const { problems, unlisted } = checkDir(path.resolve(argument));
  if (problems.length) fail(`${argument} does not match content/media.json:\n  ${problems.join("\n  ")}`);
  for (const file of readManifest()) console.log(`media: ok  ${file.name}  ${megabytes(file.bytes)}  ${file.sha256}`);
  if (unlisted.length) console.warn(`media: ${argument} also holds ${unlisted.join(", ")}, which content/media.json does not list; nginx serves it all the same`);
} else if (command === "manifest") {
  if (!argument) fail("usage: node scripts/media.ts manifest <folder>");
  const dir = path.resolve(argument);
  if (!fs.existsSync(dir) || !fs.statSync(dir).isDirectory()) fail(`${dir} is not a folder`);
  const files: MediaFile[] = fs
    .readdirSync(dir)
    .filter((name) => !name.startsWith(".") && fs.statSync(path.join(dir, name)).isFile())
    .sort()
    .map((name) => ({ name, bytes: fs.statSync(path.join(dir, name)).size, sha256: sha256File(path.join(dir, name)) }));
  if (!files.length) fail(`${dir} holds no files`);
  const existing = JSON.parse(fs.readFileSync(MANIFEST_FILE, "utf8")) as { note?: string };
  fs.writeFileSync(MANIFEST_FILE, `${JSON.stringify({ note: existing.note, files }, null, 2)}\n`);
  console.log(`media: wrote ${files.length} files to content/media.json`);
} else {
  fail("usage: node scripts/media.ts sync | fetch | verify <folder> | manifest <folder>");
}
