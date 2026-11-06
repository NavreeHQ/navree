/**
 * How fast whole screens arrive, for `/performance/`: the figures the example
 * apps measured at their full size, read from the files that own them and
 * never typed again. The store's browser suite writes `content/showcase/
 * store.json`; the inbox's writes `src/components/showcase/inbox-numbers.ts`;
 * the board keeps its seed's constants and its measurements in
 * `board-numbers.ts`. A file that changes shape fails the build.
 */
import store from "@content/showcase/store.json";

import { boardNumbers } from "@/components/showcase/board-numbers";
import { inboxNumbers } from "@/components/showcase/inbox-numbers";
import { appConstant } from "@/components/showcase/app-constants";
import { indian } from "@/lib/format";

/** One screen of an example app, measured in a browser. */
export type Screen = {
  /** The screen's name: "Orders". */
  name: string;
  /** What is behind it, in words: "2,00,000 orders". */
  behind: string;
  /** Largest paint on a laptop, in milliseconds. */
  desktopMs: number;
  /** Largest paint with a phone's screen, a CPU four times slower and Slow 4G; null where the app was not measured that way. */
  phoneMs: number | null;
  /** Everything the page loaded, compressed, in bytes. */
  bytes: number;
};

export type AppScreens = {
  app: string;
  href: string;
  /** The day it was measured, `2026-11-04`. */
  measured: string;
  screens: Screen[];
};

/** A number that is a number, or the build fails with the file named. */
function count(value: unknown, what: string): number {
  const n = typeof value === "string" ? Number(value.replace(/,/g, "")) : value;
  if (typeof n !== "number" || !Number.isFinite(n)) throw new Error(`screens: ${what} is not a number`);
  return n;
}

/** The store, at its full size: one row per page of `content/showcase/store.json`. */
function storeScreens(): AppScreens {
  const orders = count(store.rows.orders, "the store's orders");
  return {
    app: "The store",
    href: "/showcase/store/",
    measured: store.measured_on,
    screens: store.pages.map((page) => ({
      name: page.name,
      behind: `${indian(orders)} orders`,
      desktopMs: count(page.desktop.lcp_ms, `the store's ${page.name} paint`),
      phoneMs: count(page.phone.lcp_ms, `the store's ${page.name} paint on a phone`),
      bytes: count(page.desktop.total_bytes, `the store's ${page.name} bytes`),
    })),
  };
}

/** The support inbox: its first screen and an open conversation. */
function inboxScreens(): AppScreens {
  const messages = count(inboxNumbers.messages, "the inbox's messages");
  const conversations = appConstant("apps/inbox-demo/src/scale.rs", "FULL");
  const behind = `${indian(conversations)} conversations, ${indian(messages)} messages`;
  return {
    app: "The support inbox",
    href: "/showcase/inbox/",
    measured: inboxNumbers.measured,
    screens: [
      {
        name: "The first screen",
        behind,
        desktopMs: count(inboxNumbers.firstScreen.largest_paint_ms, "the inbox's first paint"),
        phoneMs: null,
        bytes: count(inboxNumbers.firstScreen.bytes_total, "the inbox's first screen bytes"),
      },
      {
        name: "An open conversation",
        behind,
        desktopMs: count(inboxNumbers.openThread.largest_paint_ms, "the inbox's conversation paint"),
        phoneMs: null,
        bytes: count(inboxNumbers.openThread.bytes_total, "the inbox's conversation bytes"),
      },
    ],
  };
}

/** Every app with screens measured, in the order the page shows them. */
export function screenTables(): AppScreens[] {
  return [storeScreens(), inboxScreens()];
}

/** How the store and the inbox were measured, in words for the page. */
export function screenMethod(): { machine: string; storeMethod: string; phone: string } {
  const machine = store.machine.replace(/\((?:R|TM)\)/g, "").replace(/\s+/g, " ").replace(/, win32$/, "").trim();
  return {
    machine,
    storeMethod: "a release build on SQLite, Chromium with the cache off, the median of five loads of each page",
    phone: "390 px wide, a CPU four times slower and Slow 4G (150 ms, 1.6 Mbit/s)",
  };
}

/** The board, which was measured for its page and its nodes, not for a paint. */
export function boardFigures() {
  return {
    cards: boardNumbers.cards,
    firstPageBytes: boardNumbers.firstPage.htmlBytes,
    firstPageBrotliKb: boardNumbers.firstPage.brotliKb,
    nodes: boardNumbers.nodes,
    cap: boardNumbers.cap,
    measured: boardNumbers.measured,
  };
}
