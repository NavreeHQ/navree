// Turns the runs into medians with their ranges, and the medians into the report: a table
// per screen and profile, the ratio of Navree to React, and a plain line for every
// measure where one is ahead of the other. Losses are listed first and are not softened.
import { bytesText, msText, summarize } from "./stats.mjs";

/** The measures of a screen, in the order of the tables. Lower is better for all of them. */
function measuresOf(screen) {
  const bytes = (key, label, phrase) => ({ key, label, kind: "bytes", phrase });
  const time = (key, label, phrase) => ({ key, label, kind: "ms", phrase });
  return [
    bytes("bytes.html", "HTML sent", "sends fewer bytes of HTML"),
    bytes("bytes.js", "JavaScript sent", "sends fewer bytes of JavaScript"),
    bytes("bytes.css", "CSS sent", "sends fewer bytes of CSS"),
    bytes("bytes.fonts", "Fonts sent", "sends fewer bytes of fonts"),
    bytes("bytes.other", "Other files sent", "sends fewer bytes of other files"),
    bytes("bytes.total", "Everything sent", "sends fewer bytes in all"),
    bytes("jsDecoded", "JavaScript once decompressed", "ships less JavaScript once it is decompressed"),
    time("fcp", "First contentful paint", "first contentful paint"),
    time("lcp", "Largest contentful paint", "largest contentful paint"),
    time("tbt", "Main thread blocked after first paint", "keeping the main thread free after first paint"),
    time("firstInteraction", `First interaction works (${screen.first.label.toLowerCase()})`, `getting the first interaction (${screen.first.label.toLowerCase()}) working`),
    ...screen.chain.map((step) => time(`step.${step.key}`, step.label, `the step "${step.label.toLowerCase()}"`)),
    bytes("interactionBytes", "Bytes fetched by the steps", "fetches fewer bytes during the steps"),
  ];
}

/** The number a run holds for a measure, or undefined. */
function valueOf(run, key) {
  if (key.startsWith("bytes.")) return run.load?.[key.slice(6)];
  if (key.startsWith("step.")) return run.interact?.steps?.[key.slice(5)];
  if (key === "firstInteraction" || key === "interactionBytes") return run.interact?.[key];
  return run.load?.[key];
}

/** summary[screen][profile][entrant][measure] = { n, median, min, max, values } */
export function summarise(results) {
  const summary = {};
  for (const screen of results.screens) {
    summary[screen.key] = {};
    for (const profile of results.profiles) {
      summary[screen.key][profile] = {};
      for (const entrant of Object.keys(results.entrants)) {
        const runs = results.runs.filter((run) => run.screen === screen.key && run.profile === profile && run.entrant === entrant);
        const cell = {};
        for (const measure of measuresOf(screen)) {
          const found = summarize(runs.map((run) => valueOf(run, measure.key)));
          if (found) cell[measure.key] = found;
        }
        summary[screen.key][profile][entrant] = cell;
      }
    }
  }
  return summary;
}

const text = (measure, value) => (measure.kind === "bytes" ? bytesText(value) : msText(value));

const cell = (measure, found) => (found ? `${text(measure, found.median)} (${text(measure, found.min)} to ${text(measure, found.max)})` : "not measured");

const ratioText = (navree, react) => (react > 0 ? (navree / react).toFixed(2) : navree > 0 ? "more than 0" : "1.00");

/** Who is ahead on a measure: "navree", "react" or null when the two are too close to tell. */
function ahead(measure, navree, react) {
  const difference = Math.abs(navree - react);
  const floor = measure.kind === "bytes" ? 256 : 5;
  if (difference < floor || difference < 0.05 * Math.max(navree, react)) return null;
  return navree < react ? "navree" : "react";
}

/** "React is faster at first contentful paint by 120 ms (Navree 450 ms, React 330 ms, ratio 1.36)". */
function sentence(measure, who, navree, react, names) {
  const amount = text(measure, Math.abs(navree - react));
  const lead = measure.kind === "bytes" ? `${names[who]} ${measure.phrase}` : `${names[who]} is faster at ${measure.phrase}`;
  return `${lead} by ${amount} (${names.navree} ${text(measure, navree)}, ${names.react} ${text(measure, react)}, ratio ${ratioText(navree, react)})`;
}

/** The lines for one table, each with who is ahead. */
function verdicts(measures, navree, react, short) {
  const lines = [];
  for (const measure of measures) {
    const a = navree[measure.key];
    const b = react[measure.key];
    if (!a || !b) continue;
    const who = ahead(measure, a.median, b.median);
    if (who) lines.push({ who, line: sentence(measure, who, a.median, b.median, short) });
  }
  return lines;
}

function table(measures, navree, react, names) {
  const rows = [`| Measure | ${names.navree} | ${names.react} | Ratio ${names.navree} to ${names.react} |`, "|---|---|---|---|"];
  for (const measure of measures) {
    const a = navree[measure.key];
    const b = react[measure.key];
    rows.push(`| ${measure.label} | ${cell(measure, a)} | ${cell(measure, b)} | ${a && b ? ratioText(a.median, b.median) : "-"} |`);
  }
  return rows.join("\n");
}

/** The report as Markdown. */
export function markdown(results, summary) {
  const names = { navree: results.entrants.navree.label, react: results.entrants.react.label };
  const short = { navree: results.entrants.navree.short, react: results.entrants.react.short };
  const sections = [];
  const losses = [];
  const wins = [];
  const body = [];
  for (const screen of results.screens) {
    body.push(`## ${screen.title}`);
    const measures = measuresOf(screen);
    for (const profile of results.profiles) {
      const navree = summary[screen.key][profile].navree;
      const react = summary[screen.key][profile].react;
      body.push(`### ${screen.title}, ${results.profileLabels[profile]}`, table(measures, navree, react, names));
      const found = verdicts(measures, navree, react, short);
      for (const item of found) {
        const entry = `${screen.title}, ${profile}: ${item.line}`;
        (item.who === "react" ? losses : wins).push(entry);
      }
      const here = found.filter((item) => item.who === "react").map((item) => `- ${item.line}`);
      body.push(
        here.length
          ? `Where ${short.navree} loses on this screen and profile:\n\n${here.join("\n")}`
          : `${short.navree} loses on none of these measures on this screen and profile.`,
      );
    }
  }

  sections.push(`# Speed comparison: ${names.navree} and ${names.react}`);
  sections.push(
    [
      `Measured ${results.generatedAt} on ${results.host.cpu} (${results.host.cores} cores, ${results.host.memoryGb} GB, ${results.host.os}), with Chromium ${results.host.chromium} driven by Playwright ${results.host.playwright}.`,
      `${results.rounds} interleaved rounds: round 1 loads every screen of both builds, then round 2, and so on, alternating which build goes first. Every load is a fresh browser context with an empty cache. Each cell is the median of the rounds, with the least and greatest in brackets. The ratio is ${names.navree} divided by ${names.react}; below 1.00 is better for ${names.navree}, above 1.00 is better for ${names.react}. A difference under 5 percent, or under 256 bytes or 5 ms, is called no difference.`,
      "`README.md` says what each measure means and how the two builds are made.",
    ].join("\n\n"),
  );
  sections.push(
    `## Where ${short.navree} loses\n\n${losses.length ? losses.map((line) => `- ${line}`).join("\n") : "Nothing was measured where it is behind by more than the margin above."}`,
  );
  sections.push(
    `## Where ${short.navree} is ahead\n\n${wins.length ? wins.map((line) => `- ${line}`).join("\n") : "Nothing was measured where it is ahead by more than the margin above."}`,
  );
  sections.push(...body);
  if (results.failures.length) {
    const lines = results.failures.map((failure) => `- Round ${failure.round}, ${failure.screen}, ${failure.entrant}, ${failure.profile}, ${failure.phase}: ${failure.message}`);
    sections.push(`## Loads that failed\n\nThese loads did not finish and are not in the medians above.\n\n${lines.join("\n")}`);
  }
  return sections.join("\n\n") + "\n";
}
