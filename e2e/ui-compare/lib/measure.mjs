// One load of one screen, measured. A load is made fresh each time: a new browser
// context with an empty cache, the profile's slowdown set through the DevTools protocol
// before the first request, and the in-page recorder (inpage.js) running before the
// page's own script.
//
// A cell (one screen, one build, one profile, one round) takes two loads, because input
// stops the browser reporting the largest paint:
//   1. the paint load: nothing is pressed; bytes, first paint, largest paint, blocking.
//   2. the interaction load: input goes to the screen every 60 ms from the moment the
//      response starts until the first interaction works, then each step is timed.
import { readFileSync } from "node:fs";
import { sleep } from "./stats.mjs";

const INPAGE = readFileSync(new URL("./inpage.js", import.meta.url), "utf8");

export const VIEWPORT = { width: 1280, height: 800 };

export const PROFILES = {
  mobile: {
    label: "Mobile: 4x CPU slowdown and Slow 4G (1.6 Mbit/s down, 750 kbit/s up, 150 ms latency)",
    cpu: 4,
    network: { latency: 150, down: 1_600_000 / 8, up: 750_000 / 8 },
  },
  desktop: {
    label: "Desktop: no slowdown",
    cpu: 1,
    network: null,
  },
};

/** A step that did not reach its expected state, or could not be done. */
export class StepError extends Error {
  constructor(message, detail) {
    super(message);
    this.name = "StepError";
    this.detail = detail;
  }
}

/** Answers `value` after `ms`, without keeping the process alive for it. */
const after = (ms, value) =>
  new Promise((done) => {
    setTimeout(() => done(value), ms).unref();
  });

/** Runs `work`; answers null instead of throwing or taking longer than `ms` (a page that is still loading throws for any call). */
async function quietly(work, ms = 30000) {
  try {
    return await Promise.race([work(), after(ms, null)]);
  } catch {
    return null;
  }
}

// ------------------------------------------------------------ network

/** Counts the bytes of every request the page makes, from the protocol's own accounting. */
async function watchNetwork(context, page, profile) {
  const client = await context.newCDPSession(page);
  const requests = new Map();
  let inflight = 0;
  let lastChange = Date.now();
  client.on("Network.requestWillBeSent", (event) => {
    if (event.redirectResponse) {
      const known = requests.get(event.requestId);
      if (known) known.url = event.request.url;
      return;
    }
    requests.set(event.requestId, { url: event.request.url, type: event.type || "Other", encoded: 0, decoded: 0, done: false, failed: false });
    inflight += 1;
    lastChange = Date.now();
  });
  client.on("Network.responseReceived", (event) => {
    const request = requests.get(event.requestId);
    if (request && event.type) request.type = event.type;
  });
  client.on("Network.dataReceived", (event) => {
    const request = requests.get(event.requestId);
    if (request) request.decoded += event.dataLength;
  });
  const end = (event, failed) => {
    const request = requests.get(event.requestId);
    if (!request || request.done) return;
    request.done = true;
    request.failed = failed;
    if (!failed) request.encoded = event.encodedDataLength;
    inflight -= 1;
    lastChange = Date.now();
  };
  client.on("Network.loadingFinished", (event) => end(event, false));
  client.on("Network.loadingFailed", (event) => end(event, true));
  await client.send("Network.enable");
  if (profile.network) {
    await client.send("Network.emulateNetworkConditions", {
      offline: false,
      latency: profile.network.latency,
      downloadThroughput: profile.network.down,
      uploadThroughput: profile.network.up,
    });
  }
  if (profile.cpu > 1) await client.send("Emulation.setCPUThrottlingRate", { rate: profile.cpu });

  const KIND = { Document: "html", Script: "js", Stylesheet: "css", Font: "fonts" };
  return {
    /** Bytes sent so far by kind, and what the scripts came to once decompressed. The browser's own request for a site icon is left out. */
    tally() {
      const sum = { html: 0, js: 0, css: 0, fonts: 0, other: 0, total: 0, jsDecoded: 0, requests: 0 };
      for (const request of requests.values()) {
        if (!request.done || request.failed || /favicon/i.test(request.url)) continue;
        sum[KIND[request.type] || "other"] += request.encoded;
        sum.total += request.encoded;
        sum.requests += 1;
        if (request.type === "Script") sum.jsDecoded += request.decoded;
      }
      return sum;
    },
    /** Waits until nothing has been in flight for `quiet` ms. */
    async idle(quiet = 500, limit = 30000) {
      const until = Date.now() + limit;
      while (Date.now() < until) {
        if (inflight <= 0 && Date.now() - lastChange >= quiet) return true;
        await sleep(50);
      }
      return false;
    },
  };
}

async function openPage(browser, profile, init) {
  const context = await browser.newContext({
    viewport: VIEWPORT,
    deviceScaleFactor: 1,
    locale: "en-US",
    timezoneId: "Asia/Kolkata",
    colorScheme: "light",
    serviceWorkers: "block",
  });
  const page = await context.newPage();
  page.setDefaultTimeout(30000);
  await page.addInitScript({ content: init });
  const net = await watchNetwork(context, page, profile);
  return { page, net, close: () => context.close().catch(() => {}) };
}

// ------------------------------------------------------------ input

/** One try at an action. Answers false when the control it needs is not on the screen yet. */
async function tryOnce(page, action) {
  if (action.kind === "key") {
    await page.keyboard.press(action.key);
    return true;
  }
  if (action.kind === "click") {
    const point = await page.evaluate((spec) => window.__cmp.pointOf(spec), { sel: action.sel, hasText: action.hasText });
    if (!point) return false;
    await page.mouse.click(point.x, point.y);
    return true;
  }
  const focused = await page.evaluate((spec) => window.__cmp.focusSelect(spec), { sel: action.sel });
  if (!focused) return false;
  if (action.kind === "type") await page.keyboard.type(action.text);
  else await page.keyboard.press("Backspace");
  return true;
}

/** Does an action, waiting for the control it needs to be drawn. */
async function perform(page, action, limit) {
  const until = Date.now() + limit;
  while (!(await tryOnce(page, action))) {
    if (Date.now() > until) throw new StepError(`there was nothing to ${action.kind} at ${action.sel}${action.hasText ? ` with the text "${action.hasText}"` : ""}`);
    await sleep(50);
  }
}

/**
 * One timed step: arms the in-page wait, does the action, and answers the time from the
 * input event to the second frame drawn after the expected state appeared. With
 * `allowAlready`, a state that is already on the screen ends the step with null.
 */
export async function runStep(page, entrant, step, { allowAlready = false, limit = 20000 } = {}) {
  const action = step.act(entrant);
  const probe = step.expect(entrant);
  const already = await page.evaluate((args) => window.__cmp.arm(args.probe, args.limit), { probe, limit });
  if (already) {
    if (allowAlready) return null;
    throw new StepError(`"${step.label}": the state it should reach was on the screen before the step began`, probe);
  }
  try {
    await perform(page, action, limit);
    const result = await Promise.race([
      page.evaluate(() => window.__cmp.pending),
      after(limit + 10000, { ok: false, reason: "no answer from the page" }),
    ]);
    if (!result.ok) {
      throw new StepError(`"${step.label}": the page did not reach the expected state within ${Math.round(limit / 1000)} s`, result.detail ?? result.reason);
    }
    if (result.started === null) throw new StepError(`"${step.label}": no input event was seen before the state appeared`);
    return result.drawn - result.started;
  } catch (error) {
    if (error instanceof StepError) throw error;
    throw new StepError(`"${step.label}": ${error.message.split("\n")[0]}`);
  }
}

/** Lets animations and requests finish between steps. */
async function calm(view, pause = 250) {
  await view.net.idle(300, 15000);
  await quietly(() => view.page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)))));
  await sleep(pause);
}

// ------------------------------------------------------------ the two loads

/** The paint load. */
export async function measureLoad(browser, url, profile) {
  const view = await openPage(browser, profile, INPAGE);
  try {
    await view.page.goto(url, { waitUntil: "load", timeout: 120000 });
    await view.net.idle(500, 30000);
    const until = Date.now() + 15000;
    let paint = await view.page.evaluate(() => window.__cmp.paint());
    while (paint.fcp === null && Date.now() < until) {
      await sleep(100);
      paint = await view.page.evaluate(() => window.__cmp.paint());
    }
    // The last paint and the last long task are reported a moment after they happen.
    await sleep(500);
    paint = await view.page.evaluate(() => window.__cmp.paint());
    if (paint.fcp === null) throw new StepError("the page never painted any content");
    const blocking = paint.tasks.filter(([start]) => start >= paint.fcp).reduce((sum, [, duration]) => sum + Math.max(0, duration - 50), 0);
    return { ...view.net.tally(), fcp: paint.fcp, lcp: paint.lcp, tbt: blocking };
  } finally {
    await view.close();
  }
}

/**
 * The interaction load: the first interaction, then undoing it, then every step of the
 * screen in order. Answers the time to the first interaction, the time of each step, and
 * the bytes the steps fetched.
 */
export async function measureInteractions(browser, url, profile, screen, entrant) {
  const first = screen.first;
  const arm = `\n;window.__cmp.arm(${JSON.stringify(first.expect(entrant))}, 120000);`;
  const view = await openPage(browser, profile, INPAGE + arm);
  try {
    await view.page.goto(url, { waitUntil: "commit", timeout: 120000 });
    const action = first.act(entrant);
    const deadline = Date.now() + 90000;
    let found = null;
    while (Date.now() < deadline) {
      found = await quietly(() => view.page.evaluate(() => (window.__cmp ? window.__cmp.result : null)));
      if (found) break;
      await quietly(() => tryOnce(view.page, action));
      await sleep(60);
    }
    if (!found || !found.ok) {
      throw new StepError(`"${first.label}" never worked in the first 90 s`, found?.detail);
    }
    const responseStart = await view.page.evaluate(() => window.__cmp.paint().responseStart);
    const firstInteraction = found.drawn - responseStart;

    await calm(view, 300);
    await runStep(view.page, entrant, screen.undo, { allowAlready: true });
    await calm(view);

    const before = view.net.tally();
    const steps = {};
    for (const step of screen.chain) {
      steps[step.key] = await runStep(view.page, entrant, step);
      await calm(view);
    }
    const after = view.net.tally();
    return { firstInteraction, steps, interactionBytes: after.total - before.total };
  } finally {
    await view.close();
  }
}

// ------------------------------------------------------------ parity

const show = (value) => JSON.stringify(value).slice(0, 300);

/** What differs between two lists, in a few words. */
function difference(got, expected) {
  if (!Array.isArray(got) || !Array.isArray(expected)) return `got ${show(got)}, expected ${show(expected)}`;
  const at = expected.findIndex((item, index) => JSON.stringify(item) !== JSON.stringify(got[index]));
  if (at === -1 && got.length !== expected.length) return `${got.length} items, expected ${expected.length}`;
  return `${got.length} items, expected ${expected.length}; the first difference is item ${at}: got ${show(got[at])}, expected ${show(expected[at])}`;
}

/**
 * Loads a screen once, unthrottled, and checks that it starts in the state SCREENS.md
 * describes and shows the data the model holds. Answers a list of problems.
 */
export async function checkParity(browser, url, screen, entrant) {
  const problems = [];
  const view = await openPage(browser, PROFILES.desktop, INPAGE);
  try {
    await view.page.goto(url, { waitUntil: "load", timeout: 120000 });
    const ready = screen.ready(entrant);
    const until = Date.now() + 30000;
    while (!(await view.page.evaluate((probe) => window.__cmp.holds(probe), ready))) {
      if (Date.now() > until) {
        const found = await view.page.evaluate((probe) => window.__cmp.describe(probe), ready);
        return [`the page never showed its first state: ${show(found)}`];
      }
      await sleep(100);
    }
    await view.net.idle(300, 15000);
    for (const check of screen.parity) {
      for (const step of check.steps ?? []) {
        await runStep(view.page, entrant, step);
        await calm(view, 100);
      }
      const read = typeof check.read === "function" ? check.read(entrant) : check.read;
      const got = await view.page.evaluate(read);
      if (JSON.stringify(got) !== JSON.stringify(check.expect)) problems.push(`${check.label}: ${difference(got, check.expect)}`);
      if (check.after) {
        await runStep(view.page, entrant, check.after, { allowAlready: true });
        await calm(view, 100);
      }
    }
  } catch (error) {
    problems.push(error instanceof StepError ? `${error.message}${error.detail ? ` (${show(error.detail)})` : ""}` : String(error.message || error));
  } finally {
    await view.close();
  }
  return problems;
}
