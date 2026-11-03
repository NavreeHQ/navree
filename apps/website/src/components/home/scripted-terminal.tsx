"use client";

import { Fragment, useEffect, useRef, useState, type ReactNode } from "react";

import { cn } from "@/lib/utils";

/** A command (typed after a `$` prompt) or a line of output (fades in). */
export type TermLine = { cmd: string; className?: string } | { out: ReactNode; className?: string };

/** Text with a line-break opportunity after each `/`, so a long URL wraps at a path segment. Adds no characters. */
function breakable(text: string): ReactNode {
  const parts = text.split("/");
  return parts.map((part, i) => (
    <Fragment key={i}>
      {part}
      {i < parts.length - 1 ? (
        <>
          /<wbr />
        </>
      ) : null}
    </Fragment>
  ));
}

function Caret({ className }: { className?: string }) {
  return (
    <span
      aria-hidden
      className={cn("ml-px inline-block h-[1.15em] w-[0.6em] translate-y-[0.2em] bg-fg-2 motion-safe:animate-[caret_1.05s_steps(1)_infinite]", className)}
    />
  );
}

/**
 * A terminal that plays a session once it scrolls into view: commands type
 * themselves, output fades in line by line, and a prompt waits at the end.
 *
 * Every line is in the HTML from the start (screen readers and search read
 * the whole session), so the server and the first client render agree.
 * Long lines wrap instead of running off the edge: a command wraps at its
 * path segments, under the prompt, and keeps every character, so what you
 * copy is the command as written.
 * Hiding uses `motion-safe:` classes only: with reduced motion the session
 * is simply shown, complete and still, and nothing is animated.
 */
export function ScriptedTerminal({
  title,
  lines,
  className,
  typeMs = 34,
}: {
  title?: string;
  lines: TermLine[];
  className?: string;
  /** Milliseconds per typed character. */
  typeMs?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [started, setStarted] = useState(false);
  const [pos, setPos] = useState({ line: 0, char: 0 });
  const done = pos.line >= lines.length;

  // Start once the terminal is on screen, unless motion is reduced.
  useEffect(() => {
    const el = ref.current;
    if (!el || window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const io = new IntersectionObserver(
      ([entry]) => {
        if (entry?.isIntersecting) {
          setStarted(true);
          io.disconnect();
        }
      },
      { threshold: 0.35 },
    );
    io.observe(el);
    return () => io.disconnect();
  }, []);

  // Advance one character or one line at a time.
  useEffect(() => {
    if (!started || done) return;
    const line = lines[pos.line];
    let delay = 110;
    let next = { line: pos.line + 1, char: 0 };
    if ("cmd" in line) {
      if (pos.char < line.cmd.length) {
        delay = pos.char === 0 ? 420 : typeMs;
        next = { line: pos.line, char: pos.char + 1 };
      } else {
        delay = 260;
      }
    }
    const timer = setTimeout(() => setPos(next), delay);
    return () => clearTimeout(timer);
  }, [started, done, pos, lines, typeMs]);

  return (
    <div ref={ref} className={cn("w-full overflow-hidden rounded-2xl border border-line bg-ink-1 shadow-bezel", className)}>
      <div className="flex h-10 items-center gap-3 border-b border-line px-4">
        <div className="flex gap-1.5" aria-hidden>
          <span className="size-2.5 rounded-full bg-line-2" />
          <span className="size-2.5 rounded-full bg-line-2" />
          <span className="size-2.5 rounded-full bg-line-2" />
        </div>
        {title ? <span className="font-mono text-xs text-fg-3">{title}</span> : null}
      </div>
      <pre className="@container p-4 font-mono text-[0.8125rem] leading-relaxed whitespace-pre-wrap [overflow-wrap:anywhere] sm:p-5">
        <code className="grid gap-y-1">
          {lines.map((line, j) => {
            const active = started && !done && j === pos.line;
            const shown = done || j < pos.line || (active && "cmd" in line);
            if ("cmd" in line) {
              const typed = active ? pos.char : line.cmd.length;
              return (
                <span key={j} className={cn("block pl-[2ch] -indent-[2ch] text-fg", !shown && "motion-safe:opacity-0", line.className)}>
                  <span className="text-fg-3 select-none">$ </span>
                  {breakable(line.cmd.slice(0, typed))}
                  <span className={active ? "motion-safe:opacity-0" : undefined}>{breakable(line.cmd.slice(typed))}</span>
                  {active ? <Caret /> : null}
                </span>
              );
            }
            return (
              <span
                key={j}
                className={cn(
                  "block text-fg-3 transition-[opacity,transform] duration-300 ease-(--ease-out-quint)",
                  !shown && "motion-safe:-translate-y-1 motion-safe:opacity-0",
                  line.className,
                )}
              >
                {line.out}
              </span>
            );
          })}
          <span aria-hidden className={cn("mt-1 block", !done && "motion-safe:opacity-0")}>
            <span className="text-fg-3">$ </span>
            <Caret />
          </span>
        </code>
      </pre>
    </div>
  );
}
