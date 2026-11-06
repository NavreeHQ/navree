import type { ReactNode } from "react";

import { scenarios, type Line } from "@/components/ai/scenarios";

import { BlockHeading } from "./parts";

/** The session this block lays out: the first, which ends in a fix and a green check. */
const session = scenarios.find((scenario) => scenario.id === "debug");

/** The `n`th line of the session, which must be of kind `kind`: a session that has changed shape fails the build. */
function line<K extends Line["kind"]>(n: number, kind: K): Extract<Line, { kind: K }> {
  const found = session?.lines[n];
  if (!found || found.kind !== kind) throw new Error(`session-still: line ${n} of the "debug" session is not a ${kind}`);
  return found as Extract<Line, { kind: K }>;
}

function Result({ lines }: { lines: ReactNode[] }) {
  return (
    <div className="mt-1 ml-1.5 border-l border-line-2 pl-3.5 text-fg-2">
      {lines.map((text, i) => (
        <div key={i} className="[overflow-wrap:anywhere]">
          {text}
        </div>
      ))}
    </div>
  );
}

/** A Navigator call or a command, with what came back. */
function Call({ line: l }: { line: Extract<Line, { kind: "tool" | "run" }> }) {
  return (
    <div>
      <div className="flex flex-wrap items-baseline gap-x-0.5">
        {l.kind === "tool" ? (
          <>
            <span className="text-fg-3">navree · </span>
            <span className="text-fg">{l.name}</span>
            <span className="text-fg-3">({l.args ?? ""})</span>
          </>
        ) : (
          <>
            <span className="text-fg">{l.verb}</span>
            <span className="text-fg-3">(</span>
            <span className="text-fg-1">{l.detail}</span>
            <span className="text-fg-3">)</span>
          </>
        )}
      </div>
      {l.result ? <Result lines={l.result} /> : null}
    </div>
  );
}

/**
 * One fix, laid out as the steps it took: the request, the question put to
 * Navigator, the cause it found, the command that fixed it and the check that
 * ended green. It reads the same lines as the animated session in the hero
 * (`src/components/ai/scenarios.tsx`), so the two cannot disagree.
 */
export function SessionStill() {
  const ask = line(0, "prompt");
  const error = line(1, "tool");
  const cause = line(2, "tool");
  const fix = line(3, "run");
  const check = line(4, "run");
  const reply = line(5, "reply");

  const steps: { title: string; text: string; body: ReactNode }[] = [
    {
      title: "You ask in plain words",
      text: "The page is empty and the API answers 500. You don't say why.",
      body: (
        <div className="flex gap-2 text-fg">
          <span aria-hidden className="text-fg-3">
            ›
          </span>
          <span>{ask.text}</span>
        </div>
      ),
    },
    {
      title: "It asks the running app",
      text: "Navigator's last_error returns the failed request and its log lines, so the agent starts from the real error, not a guess.",
      body: <Call line={error} />,
    },
    {
      title: "It finds the cause",
      text: "describe_schema shows the database as it is: a migration the code needs hasn't run.",
      body: <Call line={cause} />,
    },
    {
      title: "It makes the fix",
      text: "One command, the one the project already uses.",
      body: <Call line={fix} />,
    },
    {
      title: "It checks, and says what it did",
      text: "The same request, answered again, and a short account of the cause and the fix.",
      body: (
        <>
          <Call line={check} />
          <p className="mt-3 font-sans text-[0.875rem] leading-relaxed text-pretty text-fg-2">{reply.text}</p>
        </>
      ),
    },
  ];

  return (
    <div>
      <BlockHeading label="last_error" title="One fix, step by step" />
      <p className="mt-5 max-w-2xl leading-relaxed text-pretty text-fg-2">
        The first session at the top of this page, laid out as the five steps it took. The tools and the shape of their
        answers are Navigator&rsquo;s; the app is a CRM.
      </p>
      <ol className="mt-8 grid grid-cols-1 gap-px overflow-hidden rounded-2xl border border-line bg-line">
        {steps.map((step, i) => (
          <li
            key={step.title}
            className="grid grid-cols-1 gap-x-10 gap-y-4 bg-ink-0 p-5 sm:p-6 md:grid-cols-[minmax(0,19rem)_minmax(0,1fr)] lg:px-8"
          >
            <div className="flex min-w-0 items-baseline gap-4">
              <span className="font-mono text-xs text-accent numeric">{String(i + 1).padStart(2, "0")}</span>
              <div className="min-w-0">
                <h4 className="font-medium text-fg">{step.title}</h4>
                <p className="mt-1 text-sm leading-relaxed text-pretty text-fg-3">{step.text}</p>
              </div>
            </div>
            <div className="min-w-0 rounded-xl border border-line bg-ink-1 px-4 py-3 font-mono text-[0.78rem] leading-[1.7] sm:text-[0.8125rem]">
              {step.body}
            </div>
          </li>
        ))}
      </ol>
    </div>
  );
}
