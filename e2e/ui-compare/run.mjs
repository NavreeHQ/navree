#!/usr/bin/env node
// Measures four screens built with Navree UI against the same four built with React and
// shadcn/ui, in Chromium, in interleaved rounds. README.md says what is measured and how
// to read the report; SCREENS.md says what the screens are.
//
//   cd e2e/ui-compare && npm ci && node run.mjs
//
// Options:
//   --rounds N            rounds to run (default 5)
//   --screens a,b         only these screens: table, select, dates, tabs
//   --profiles a,b        only these profiles: mobile, desktop
//   --skip-react-build    use react/dist as it is
//
// NAVREE_UI_URL names a Navree UI server that is already running; without it the proof
// app is built and started on a free port, and stopped when the run ends.
//
// The exit code is 0 whoever is ahead. It is 1 when the harness fails, when the two
// builds of a screen do not agree, or when a step never reached its expected state.
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import os from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { PROFILES, StepError, checkParity, measureInteractions, measureLoad } from "./lib/measure.mjs";
import { markdown, summarise } from "./lib/report.mjs";
import { SCREENS } from "./lib/screens.mjs";
import { buildReact, run, serveFolder, startNavree } from "./lib/servers.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..", "..");
const require = createRequire(import.meta.url);

const log = (message) => console.log(`[${new Date().toISOString().slice(11, 19)}] ${message}`);

// ---------------------------------------------------------------- options

function parseOptions(argv) {
  const options = { rounds: 5, screens: SCREENS.map((screen) => screen.key), profiles: Object.keys(PROFILES), skipReactBuild: false };
  for (let at = 0; at < argv.length; at++) {
    const next = () => {
      if (at + 1 >= argv.length) throw new Error(`${argv[at]} needs a value`);
      return argv[++at];
    };
    switch (argv[at]) {
      case "--rounds":
        options.rounds = Number(next());
        if (!Number.isInteger(options.rounds) || options.rounds < 1) throw new Error("--rounds takes a whole number from 1");
        break;
      case "--screens":
        options.screens = next().split(",");
        break;
      case "--profiles":
        options.profiles = next().split(",");
        break;
      case "--skip-react-build":
        options.skipReactBuild = true;
        break;
      default:
        throw new Error(`unknown option ${argv[at]}; see the head of run.mjs`);
    }
  }
  for (const key of options.screens) if (!SCREENS.some((screen) => screen.key === key)) throw new Error(`no screen called ${key}`);
  for (const key of options.profiles) if (!PROFILES[key]) throw new Error(`no profile called ${key}`);
  return options;
}

// ---------------------------------------------------------------- cleanup

const cleanups = [];
let startedProgram = null;
let cleaning = null;

/** Stops what this run started, once, whichever way the run ends. */
function cleanUp() {
  cleaning ??= (async () => {
    for (const stop of cleanups.splice(0).reverse()) {
      try {
        await stop();
      } catch (error) {
        console.error(`While cleaning up: ${error.message || error}`);
      }
    }
  })();
  return cleaning;
}

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => {
    console.error(`\n${signal}: stopping what this run started.`);
    cleanUp().finally(() => process.exit(130));
  });
}
// The last resort when the process ends some other way: the proof app, if this run started it.
process.on("exit", () => {
  if (startedProgram && startedProgram.exitCode === null) startedProgram.kill();
});

// ---------------------------------------------------------------- browser

async function launchBrowser() {
  const options = {
    headless: true,
    args: ["--disable-background-timer-throttling", "--disable-renderer-backgrounding", "--disable-backgrounding-occluded-windows"],
  };
  try {
    return await chromium.launch(options);
  } catch (error) {
    if (!/Executable doesn't exist/.test(String(error.message))) throw error;
    log("Chromium is not installed for this Playwright; installing it ...");
    run(process.execPath, [join(dirname(require.resolve("playwright")), "cli.js"), "install", "chromium"], { cwd: here });
    return chromium.launch(options);
  }
}

// ---------------------------------------------------------------- the run

/** What went wrong, in a few lines. */
function describeError(error) {
  const detail = error instanceof StepError && error.detail !== undefined ? ` ${JSON.stringify(error.detail).slice(0, 400)}` : "";
  return `${String(error.message || error).split("\n")[0]}${detail}`;
}

async function main() {
  const options = parseOptions(process.argv.slice(2));
  const entrants = JSON.parse(readFileSync(join(here, "entrants.json"), "utf8"));
  const screens = SCREENS.filter((screen) => options.screens.includes(screen.key));
  for (const entrant of Object.keys(entrants)) {
    for (const screen of screens) {
      if (!entrants[entrant].routes[screen.key]) throw new Error(`entrants.json names no route for ${screen.key} on ${entrant}`);
    }
  }

  const limitMinutes = Number(process.env.COMPARE_LIMIT_MINUTES || 240);
  setTimeout(() => {
    console.error(`The run went past its limit of ${limitMinutes} minutes (COMPARE_LIMIT_MINUTES) and was stopped.`);
    cleanUp().finally(() => process.exit(1));
  }, limitMinutes * 60 * 1000).unref();

  buildReact(join(here, "react"), { log, skipBuild: options.skipReactBuild });
  const react = await serveFolder(join(here, "react", "dist"));
  cleanups.push(() => react.stop());
  log(`Serving the React pages at ${react.url}`);
  const navree = await startNavree({ root, log });
  cleanups.push(() => navree.stop());
  startedProgram = navree.child ?? null;
  const bases = { navree: navree.url, react: react.url };
  const urlOf = (entrant, screen) => bases[entrant] + entrants[entrant].routes[screen];

  for (const entrant of Object.keys(entrants)) {
    for (const screen of screens) {
      const url = urlOf(entrant, screen.key);
      const answer = await fetch(url, { signal: AbortSignal.timeout(30000) }).catch((error) => ({ status: `no answer (${error.message})` }));
      if (answer.status !== 200) throw new Error(`${url} answered ${answer.status}; the ${entrant} build of the ${screen.key} screen is not being served`);
    }
  }

  let browser = await launchBrowser();
  cleanups.push(() => browser.close().catch(() => {}));
  const chromiumVersion = browser.version();

  // The two builds of each screen must be the same screen before anything is timed.
  log("Checking that the two builds of each screen agree ...");
  const disagreements = [];
  for (const screen of screens) {
    for (const entrant of Object.keys(entrants)) {
      const problems = await checkParity(browser, urlOf(entrant, screen.key), screen, entrant);
      for (const problem of problems) disagreements.push(`${screen.key}, ${entrant}: ${problem}`);
      log(`  ${screen.key}, ${entrant}: ${problems.length ? "differs" : "agrees"}`);
    }
  }
  if (disagreements.length) {
    console.error(`\nThe screens do not agree with SCREENS.md, so nothing was measured:\n${disagreements.map((line) => `  - ${line}`).join("\n")}`);
    await cleanUp();
    process.exit(1);
  }

  const runs = [];
  const failures = [];
  for (let round = 1; round <= options.rounds; round++) {
    if (round > 1) {
      await browser.close().catch(() => {});
      browser = await launchBrowser();
    }
    // Which build goes first alternates, so neither always has the quieter machine.
    const order = round % 2 ? ["navree", "react"] : ["react", "navree"];
    for (const profile of options.profiles) {
      for (const screen of screens) {
        for (const entrant of order) {
          if (!browser.isConnected()) browser = await launchBrowser();
          const url = urlOf(entrant, screen.key);
          const cell = { round, profile, screen: screen.key, entrant, load: null, interact: null };
          const fail = (phase, error) => {
            failures.push({ round, profile, screen: screen.key, entrant, phase, message: describeError(error) });
            console.error(`  FAILED round ${round}, ${profile}, ${screen.key}, ${entrant}, ${phase}: ${describeError(error)}`);
          };
          try {
            cell.load = await measureLoad(browser, url, PROFILES[profile]);
          } catch (error) {
            fail("paint load", error);
          }
          try {
            cell.interact = await measureInteractions(browser, url, PROFILES[profile], screen, entrant);
          } catch (error) {
            fail("interaction load", error);
          }
          runs.push(cell);
          const fcp = cell.load ? `${Math.round(cell.load.fcp)} ms to first paint` : "no paint load";
          const first = cell.interact ? `${Math.round(cell.interact.firstInteraction)} ms to the first interaction` : "no interaction load";
          log(`round ${round}/${options.rounds}  ${profile.padEnd(7)}  ${screen.key.padEnd(6)}  ${entrant.padEnd(6)}  ${fcp}, ${first}`);
        }
      }
    }
  }

  const results = {
    generatedAt: new Date().toISOString().slice(0, 10),
    rounds: options.rounds,
    host: {
      os: `${os.type()} ${os.release()}`,
      cpu: os.cpus()[0]?.model.trim() ?? "unknown CPU",
      cores: os.cpus().length,
      memoryGb: Math.round(os.totalmem() / 2 ** 30),
      node: process.version,
      chromium: chromiumVersion,
      playwright: JSON.parse(readFileSync(join(dirname(require.resolve("playwright")), "package.json"), "utf8")).version,
    },
    profiles: options.profiles,
    profileLabels: Object.fromEntries(options.profiles.map((key) => [key, PROFILES[key].label])),
    entrants: Object.fromEntries(Object.entries(entrants).map(([key, value]) => [key, { label: value.label, short: value.short }])),
    screens: screens.map((screen) => ({
      key: screen.key,
      title: screen.title,
      first: { key: screen.first.key, label: screen.first.label },
      chain: screen.chain.map((step) => ({ key: step.key, label: step.label })),
    })),
    runs,
    failures,
  };
  results.summary = summarise(results);

  const folder = join(here, "results");
  mkdirSync(folder, { recursive: true });
  const dated = join(folder, `${results.generatedAt}.json`);
  writeFileSync(dated, JSON.stringify(results, null, 2) + "\n");
  copyFileSync(dated, join(folder, "latest.json"));
  const report = markdown(results, results.summary);
  writeFileSync(join(folder, "latest.md"), report);
  log(`Wrote results/${results.generatedAt}.json, results/latest.json and results/latest.md`);

  const start = report.indexOf("## Where");
  console.log(`\n${report.slice(start, report.indexOf("\n## ", report.indexOf("is ahead")))}`);

  await cleanUp();
  if (failures.length) {
    console.error(`${failures.length} load(s) failed; see the end of results/latest.md.`);
    process.exit(1);
  }
  process.exit(0);
}

main().catch(async (error) => {
  console.error(error?.stack || error);
  await cleanUp();
  process.exit(1);
});
