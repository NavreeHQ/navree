import media from "../../content/media.json" with { type: "json" };
import { site } from "../../site.config.ts";

/**
 * The launch film, shown on the homepage's hero: its files, its words and
 * its facts. The player (`components/home/film-player.tsx`), the text under
 * it and the homepage's `VideoObject` all read them from here.
 *
 * The files are not in git. The live site serves them from `/media/`, and
 * `content/media.json` lists each one with its size and SHA-256 (see
 * `scripts/media.ts`). A name here that the list does not have fails
 * `pnpm check:links`.
 */

/** The film's length in seconds, as the files measure it (64.77 s). */
const DURATION_SECONDS = 64.8;

/** The URL of one of the files `content/media.json` lists; a name it does not list fails the build. */
function mediaUrl(name: string): string {
  if (!media.files.some((file) => file.name === name)) throw new Error(`${name} is not in content/media.json`);
  return `/media/${name}`;
}

export const film = {
  name: `${site.name} launch film`,
  /** The day the film was made, as `uploadDate` in the structured data. */
  uploadDate: "2026-11-04",
  durationSeconds: DURATION_SECONDS,
  /** 1920 x 1080, 16:9, in every file. */
  width: 1920,
  height: 1080,
  poster: {
    webp: mediaUrl("navree-launch-poster.webp"),
    jpg: mediaUrl("navree-launch-poster.jpg"),
    width: 1920,
    height: 1080,
    alt: `The ${site.name} logo on black, with the line "${site.headline}" and ${site.domain} under it. The first picture of the launch film.`,
  },
  /**
   * What plays, best first. The browser takes the first it can decode:
   * AV1 in WebM where it can, else H.264 in MP4. The codec strings are the
   * files' own (AV1 Main, 10-bit, level 4.0; H.264 High, level 4.1; AAC-LC).
   */
  sources: [
    { src: mediaUrl("navree-launch-1080p.av1.webm"), type: 'video/webm; codecs="av01.0.08M.10, opus"' },
    { src: mediaUrl("navree-launch-1080p.mp4"), type: 'video/mp4; codecs="avc1.640029, mp4a.40.2"' },
  ],
  /** For a phone: the smaller H.264 file first, and the larger as a last try. */
  phoneSources: [
    { src: mediaUrl("navree-launch-720p.mp4"), type: 'video/mp4; codecs="avc1.640028, mp4a.40.2"' },
    { src: mediaUrl("navree-launch-1080p.mp4"), type: 'video/mp4; codecs="avc1.640029, mp4a.40.2"' },
  ],
  /** The file the structured data names as the film itself (`contentUrl`) and the link for a visitor whose browser cannot play it. */
  file: { src: mediaUrl("navree-launch-1080p.mp4"), type: "video/mp4" },
  /** What the film shows, in two or three plain sentences. The structured data's description too. */
  description:
    "A short film with music and no speech. One command makes a new app, and then come its API, its data and screens written in Rust. A store, a board and an inbox run live, and the film ends on the Navree logo.",
  /** Each scene in turn, for a visitor who cannot watch it. `at` is the second it starts. */
  scenes: [
    { at: 0, text: "A dark terminal and a blinking cursor. The command navree new shop is typed." },
    { at: 9, text: "One command. Navree makes a new app, and the files it wrote are listed." },
    { at: 13, text: "Your API. A controller, written in Rust." },
    { at: 15, text: "Your data. A model and its migration, for PostgreSQL, MySQL, SQLite or MongoDB." },
    { at: 18, text: "Your screens, in Rust. A CRM built with Navree UI: deals in rupees, and a deal open in a drawer with its timeline." },
    {
      at: 25,
      text: "A store, a board and an inbox, each running live. Then a date range picker, a phone number field, country flags, a command palette and charts.",
    },
    { at: 35, text: "The numbers. How many conversations and messages the support inbox holds, and how fast its first screen appears." },
    { at: 42, text: "Safe by default. A script typed into a search box shows as plain text, and the browser blocks the one that is added to the page." },
    { at: 46, text: "Built for teams, and for agents. The skills for a coding agent are installed, and Navree's MCP server lists its tools." },
    { at: 51, text: "Who built Navree, and why it has its name." },
    { at: 58, text: `The Navree logo, the line "${site.headline}" and ${site.domain}.` },
  ],
} as const;

/** `64.8` as "1:05": the length a visitor sees on the play button. */
export function lengthLabel(seconds: number): string {
  const whole = Math.round(seconds);
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}

/** `64.8` as "1 minute 5 seconds", for a screen reader. */
export function lengthSpoken(seconds: number): string {
  const whole = Math.round(seconds);
  const minutes = Math.floor(whole / 60);
  const rest = whole % 60;
  const part = (n: number, unit: string) => `${n} ${unit}${n === 1 ? "" : "s"}`;
  return [minutes ? part(minutes, "minute") : "", rest ? part(rest, "second") : ""].filter(Boolean).join(" ");
}

/** `64.8` as an ISO 8601 duration, `PT1M5S`, for the structured data. */
export function lengthIso(seconds: number): string {
  const whole = Math.round(seconds);
  const minutes = Math.floor(whole / 60);
  const rest = whole % 60;
  return `PT${minutes ? `${minutes}M` : ""}${rest || !minutes ? `${rest}S` : ""}`;
}
