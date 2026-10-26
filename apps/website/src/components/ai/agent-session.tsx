"use client";

import { useInView, useReducedMotion } from "motion/react";
import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent,
  type ReactNode,
} from "react";

import { cn } from "@/lib/utils";

/*
 * A coding agent at work in a Navree app: the user's request, the skills it
 * loads, the Navigator tools it calls (`navree mcp`) with their results, the
 * commands it runs and its answer. Lines appear one after another once the
 * panel scrolls into view, then the next scenario plays. Under
 * prefers-reduced-motion every line shows at once and nothing advances.
 *
 * The tool names and results follow /docs/ai/navigator/.
 */

type Line =
  | { kind: "prompt"; text: string }
  | { kind: "skill"; name: string }
  | { kind: "tool"; name: string; args?: string; result: ReactNode[] }
  | { kind: "run"; verb: string; detail: string; result?: ReactNode[] }
  | { kind: "reply"; text: ReactNode };

export type Scenario = { id: string; label: string; lines: Line[] };

const C = ({ children }: { children: ReactNode }) => <span className="font-mono text-[0.92em] text-fg-1">{children}</span>;

export const scenarios: Scenario[] = [
  {
    id: "debug",
    label: "Fix a 500",
    lines: [
      { kind: "prompt", text: "The deals page is empty since I pulled main, and the API returns 500. Fix it." },
      {
        kind: "tool",
        name: "last_error",
        result: [
          <>
            <span className="text-danger">500</span> GET /api/v1/deals <span className="text-fg-3">request_id=0199c4b1</span>
          </>,
          <>error: no such column: deals.expected_close_date</>,
        ],
      },
      {
        kind: "tool",
        name: "describe_schema",
        args: 'table: "deals"',
        result: [
          <>deals: id, title, amount_cents, currency, stage, position, …</>,
          <>
            pending migration: <C>m20261026_000004_add_expected_close_date</C>
          </>,
        ],
      },
      { kind: "run", verb: "Run", detail: "navree migrate", result: [<>Applied m20261026_000004_add_expected_close_date</>] },
      {
        kind: "reply",
        text: (
          <>
            The code you pulled reads <C>deals.expected_close_date</C>, but the migration that adds it hadn&rsquo;t run on
            your database. I ran <C>navree migrate</C>; <C>GET /api/v1/deals</C> answers 200 again.
          </>
        ),
      },
    ],
  },
  {
    id: "route",
    label: "Add a route",
    lines: [
      { kind: "prompt", text: "Let a manager reassign a deal to another rep, and test it." },
      { kind: "skill", name: "navree-controllers-and-routing" },
      { kind: "skill", name: "navree-access-control" },
      {
        kind: "tool",
        name: "list_routes",
        args: 'path: "/api/v1/deals"',
        result: [
          <>
            GET&nbsp;&nbsp;/api/v1/deals <span className="text-fg-3">can:read Deal</span>
          </>,
          <>
            POST /api/v1/deals/{"{id}"}/move <span className="text-fg-3">can:move Deal from id</span>
          </>,
        ],
      },
      {
        kind: "tool",
        name: "module_graph",
        args: 'module: "DealsModule"',
        result: [<>DealsModule exports DealService, DealRepository</>],
      },
      {
        kind: "run",
        verb: "Edit",
        detail: "crates/crm-deals/src/controller.rs",
        result: [
          <>
            <span className="text-accent">+</span> #[post(&quot;/{"{id}"}/reassign&quot;)] #[can(Manage, Deal, from = &quot;id&quot;)]
          </>,
        ],
      },
      {
        kind: "run",
        verb: "Run",
        detail: "cargo test -p crm-deals reassign",
        result: [
          <>
            2 passed: manager_reassigns_deal, rep_cannot_reassign <span className="text-accent">ok</span>
          </>,
        ],
      },
      {
        kind: "reply",
        text: (
          <>
            Added <C>POST /api/v1/deals/{"{id}"}/reassign</C> next to <C>move</C>, behind <C>Manage</C> on the deal, with a
            validated body and two tests.
          </>
        ),
      },
    ],
  },
];

const noop = () => () => {};
const isClient = () => true;
const isServer = () => false;

/** How long each kind of line takes before the next one starts, ms. */
const pace: Record<Line["kind"], number> = { prompt: 1500, skill: 450, tool: 1150, run: 1000, reply: 900 };
/** How long a finished scenario stays before the next plays, ms. */
const HOLD = 6500;

function LineView({ line, shown, typed }: { line: Line; shown: boolean; typed: number }) {
  const base = cn(
    "transition-[opacity,transform] duration-300 ease-(--ease-out-quint) motion-reduce:transition-none",
    shown ? "translate-y-0 opacity-100" : "translate-y-1 opacity-0",
  );

  switch (line.kind) {
    case "prompt":
      return (
        <div className={cn(base, "mb-3 rounded-lg border border-line bg-ink-2 px-3 py-2.5 text-fg")}>
          <span className="flex gap-2">
            <span aria-hidden className="text-fg-3">
              ›
            </span>
            {/* The full text sizes the box; the typed part is drawn over it. */}
            <span className="grid">
              <span className="invisible col-start-1 row-start-1">{line.text}</span>
              <span aria-hidden className="col-start-1 row-start-1">
                {line.text.slice(0, typed)}
                {shown && typed < line.text.length ? <span className="ml-px inline-block h-[1.05em] w-[0.5ch] translate-y-[0.15em] bg-fg-2" /> : null}
              </span>
              <span className="sr-only">{line.text}</span>
            </span>
          </span>
        </div>
      );
    case "skill":
      return (
        <div className={cn(base, "flex items-baseline gap-2 text-fg-3")}>
          <span aria-hidden className="w-3 shrink-0 text-center text-[0.625rem] text-fg-2">
            ◆
          </span>
          <span>
            skill <span className="text-fg-1">{line.name}</span>
          </span>
        </div>
      );
    case "tool":
      return (
        <div className={cn(base, "mt-2")}>
          <div className="flex items-baseline gap-2">
            <span aria-hidden className="relative top-[-0.05em] size-1.5 w-3 shrink-0">
              <span className="mx-auto block size-1.5 rounded-full bg-accent shadow-[0_0_6px_rgb(61_255_143/0.8)]" />
            </span>
            <span>
              <span className="text-fg-3">navree · </span>
              <span className="text-fg">{line.name}</span>
              <span className="text-fg-3">({line.args ?? ""})</span>
            </span>
          </div>
          <Result lines={line.result} />
        </div>
      );
    case "run":
      return (
        <div className={cn(base, "mt-2")}>
          <div className="flex items-baseline gap-2">
            <span aria-hidden className="w-3 shrink-0">
              <span className="mx-auto block size-1.5 rounded-full border border-fg-3" />
            </span>
            <span>
              <span className="text-fg">{line.verb}</span>
              <span className="text-fg-3">(</span>
              <span className="text-fg-1">{line.detail}</span>
              <span className="text-fg-3">)</span>
            </span>
          </div>
          {line.result ? <Result lines={line.result} /> : null}
        </div>
      );
    case "reply":
      return <p className={cn(base, "mt-3.5 font-sans text-[0.875rem] leading-relaxed text-pretty text-fg-2")}>{line.text}</p>;
  }
}

function Result({ lines }: { lines: ReactNode[] }) {
  return (
    <div className="mt-0.5 ml-1.5 border-l border-line-2 pl-[calc(0.875rem-1px)] text-fg-2">
      {lines.map((text, i) => (
        <div key={i} className="[overflow-wrap:anywhere]">
          {text}
        </div>
      ))}
    </div>
  );
}

/**
 * The animated session in a double-bezel frame. `startIndex` picks the
 * first scenario; `autoAdvance` plays the next one after a pause.
 */
export function AgentSession({
  className,
  items = scenarios,
  autoAdvance = true,
  title = "~/code/crm",
}: {
  className?: string;
  items?: Scenario[];
  autoAdvance?: boolean;
  title?: string;
}) {
  const reduced = useReducedMotion();
  const root = useRef<HTMLDivElement>(null);
  const inView = useInView(root, { amount: 0.35 });
  // Starts the first time the panel is in view; pauses while it is out of view.
  const started = useInView(root, { amount: 0.35, once: true });
  const id = useId();
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);

  const viewport = useRef<HTMLDivElement>(null);
  const lineEls = useRef<(HTMLDivElement | null)[]>([]);

  const [active, setActive] = useState(0);
  // How many lines of the active scenario are showing, and how much of the
  // prompt is typed. The first prompt is in the HTML already typed, so it
  // never flashes; the scenarios after it type theirs.
  const [step, setStep] = useState(1);
  const [typed, setTyped] = useState(Number.MAX_SAFE_INTEGER);
  // How far the transcript has scrolled up, like a terminal, to keep the newest line in view.
  const [offset, setOffset] = useState(0);
  const [width, setWidth] = useState(0);
  const [paused, setPaused] = useState(false);

  const lines = items[active].lines;
  const done = step >= lines.length;

  // The server can't know the motion preference: render as it did, then switch once hydrated.
  const hydrated = useSyncExternalStore(noop, isClient, isServer);
  const all = hydrated && reduced === true;

  // Typing the prompt.
  useEffect(() => {
    if (all || !started) return;
    const prompt = lines[0];
    if (prompt.kind !== "prompt" || typed >= prompt.text.length) return;
    const t = setTimeout(() => setTyped((n) => Math.min(prompt.text.length, n + 2)), 22);
    return () => clearTimeout(t);
  }, [all, started, typed, lines]);

  // The next line, once the one before has had its time.
  useEffect(() => {
    if (all || !started || !inView) return;
    if (!done) {
      const prompt = lines[0];
      if (step === 1 && prompt.kind === "prompt" && typed < prompt.text.length) return;
      const t = setTimeout(() => setStep((n) => n + 1), pace[lines[step - 1].kind]);
      return () => clearTimeout(t);
    }
    if (!autoAdvance || paused || items.length < 2) return;
    const t = setTimeout(() => {
      setActive((a) => (a + 1) % items.length);
      setStep(1);
      setTyped(0);
    }, HOLD);
    return () => clearTimeout(t);
  }, [all, started, inView, done, step, typed, lines, autoAdvance, paused, items.length]);

  const select = useCallback((next: number) => {
    setActive(next);
    setStep(1);
    setTyped(0);
    setPaused(true);
  }, []);

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const last = items.length - 1;
    let next = active;
    if (event.key === "ArrowRight") next = active === last ? 0 : active + 1;
    else if (event.key === "ArrowLeft") next = active === 0 ? last : active - 1;
    else return;
    event.preventDefault();
    select(next);
    tabs.current[next]?.focus();
  }


  useEffect(() => {
    const el = viewport.current;
    if (!el) return;
    const observer = new ResizeObserver(() => setWidth(el.clientWidth));
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  useLayoutEffect(() => {
    const el = viewport.current;
    const last = lineEls.current[Math.min(step, lines.length) - 1];
    if (all || !el || !last) {
      setOffset(0);
      return;
    }
    const bottom = last.offsetTop + last.offsetHeight + 20;
    setOffset(Math.max(0, bottom - el.clientHeight));
  }, [all, step, active, lines.length, width]);

  return (
    <div
      ref={root}
      className={cn("bezel", className)}
      onPointerEnter={() => setPaused(true)}
      onPointerLeave={() => setPaused(false)}
    >
      <div className="bezel-core">
        {/* Title bar: where the agent runs, and Navigator's connection. */}
        <div className="flex h-11 items-center justify-between gap-3 border-b border-line px-4">
          <div className="flex min-w-0 items-center gap-3">
            <div className="flex gap-1.5" aria-hidden>
              <span className="size-2.5 rounded-full bg-line-2" />
              <span className="size-2.5 rounded-full bg-line-2" />
              <span className="size-2.5 rounded-full bg-line-2" />
            </div>
            <span className="truncate font-mono text-xs text-fg-3">agent · {title}</span>
          </div>
          <span className="inline-flex shrink-0 items-center gap-2 rounded-full border border-line-2 bg-ink-2 px-2.5 py-0.5 font-mono text-[0.75rem] text-fg-2">
            <span aria-hidden className="size-1.5 rounded-full bg-accent shadow-[0_0_6px_rgb(61_255_143/0.8)]" />
            navree mcp
          </span>
        </div>

        {items.length > 1 ? (
          <div
            role="tablist"
            aria-label="Agent sessions"
            onKeyDown={onKeyDown}
            className="flex gap-1 border-b border-line px-2.5"
          >
            {items.map((s, i) => (
              <button
                key={s.id}
                ref={(el) => {
                  tabs.current[i] = el;
                }}
                type="button"
                role="tab"
                id={`${id}-tab-${s.id}`}
                aria-selected={i === active}
                aria-controls={`${id}-panel`}
                tabIndex={i === active ? 0 : -1}
                onClick={() => select(i)}
                className={cn(
                  "relative h-10 cursor-pointer px-2 text-[0.8125rem] font-medium transition-colors duration-200",
                  "after:absolute after:inset-x-2 after:-bottom-px after:h-px after:bg-accent after:transition-opacity",
                  i === active ? "text-fg after:opacity-100" : "text-fg-3 after:opacity-0 hover:text-fg-1",
                )}
              >
                {s.label}
              </button>
            ))}
          </div>
        ) : null}

        {/* A fixed-height window that scrolls up as lines arrive, like a terminal. */}
        <div
          ref={viewport}
          id={`${id}-panel`}
          role={items.length > 1 ? "tabpanel" : undefined}
          aria-labelledby={items.length > 1 ? `${id}-tab-${items[active].id}` : undefined}
          tabIndex={all ? 0 : undefined}
          className={cn(
            "relative h-[24rem] font-mono text-[0.78rem] leading-[1.7] sm:h-[25rem] sm:text-[0.8125rem] lg:h-[26.5rem]",
            all ? "scroll-thin overflow-y-auto" : "overflow-hidden",
            offset > 0 ? "[mask-image:linear-gradient(to_bottom,transparent,black_2.5rem)]" : "",
          )}
        >
          <div
            key={items[active].id}
            className="relative px-4 pt-4 pb-5 transition-transform duration-500 ease-(--ease-out-quint) motion-reduce:transition-none sm:px-5"
            style={{ transform: `translateY(${-offset}px)` }}
          >
            {lines.map((line, n) => (
              <div
                key={n}
                ref={(el) => {
                  lineEls.current[n] = el;
                }}
              >
                <LineView line={line} shown={all || n < step} typed={all ? Number.MAX_SAFE_INTEGER : typed} />
              </div>
            ))}
          </div>
        </div>

        <div className="flex items-center justify-between gap-3 border-t border-line px-4 py-2.5 font-mono text-[0.75rem] text-fg-3">
          <span className="truncate">12 tools · read-only · stdio</span>
          <span aria-hidden className={cn("inline-flex items-center gap-1.5", done || all ? "text-fg-3" : "text-fg-2")}>
            <span
              className={cn(
                "size-1.5 rounded-full",
                done || all ? "bg-line-2" : "animate-pulse bg-accent motion-reduce:animate-none",
              )}
            />
            {done || all ? "idle" : "working"}
          </span>
        </div>
      </div>
    </div>
  );
}
