"use client";

import { useId, useRef, useState, type KeyboardEvent } from "react";

export type BarRow = {
  id: string;
  label: string;
  /** Shown after the label in muted text, e.g. "micro". */
  note?: string;
  value: number;
  /** The value as shown at the bar's tip. */
  display: string;
  highlight?: boolean;
  /** Tooltip lines: [label, value]. */
  details: [string, string][];
};

export type TickFormat = "rps" | "ms" | "mb";

function niceStep(max: number, target = 4): number {
  const raw = max / target;
  const power = 10 ** Math.floor(Math.log10(raw));
  const unit = raw / power;
  const nice = unit <= 1 ? 1 : unit <= 2 ? 2 : unit <= 2.5 ? 2.5 : unit <= 5 ? 5 : 10;
  return nice * power;
}

function tick(n: number, format: TickFormat): string {
  if (format === "rps") {
    if (n >= 1_000_000) return `${+(n / 1_000_000).toFixed(2)}M`;
    if (n >= 1000) return `${+(n / 1000).toFixed(1)}k`;
    return String(n);
  }
  if (format === "mb") return n >= 1000 ? `${+(n / 1000).toFixed(1)} GB` : `${n} MB`;
  return `${n} ms`;
}

/**
 * A ranked horizontal bar chart: one bar per framework, the highlighted
 * one in the accent and the rest in grey (the emphasis form). Values sit
 * at the bar tips; hovering or focusing a bar shows its details. Arrow
 * keys move between bars.
 */
export function RankedBars({
  rows,
  caption,
  format,
}: {
  rows: BarRow[];
  /** Read by screen readers; the visible title is the section's heading. */
  caption: string;
  format: TickFormat;
}) {
  const id = useId();
  const container = useRef<HTMLDivElement>(null);
  const items = useRef<(HTMLLIElement | null)[]>([]);
  const [active, setActive] = useState<number | null>(null);
  const [focusIndex, setFocusIndex] = useState(0);
  const [tipPos, setTipPos] = useState<{ top: number; left: number; below: boolean } | null>(null);

  const max = Math.max(...rows.map((r) => r.value), 0) || 1;
  const step = niceStep(max);
  const top = Math.ceil(max / step) * step;
  const ticks: number[] = [];
  for (let t = 0; t <= top + step / 2; t += step) ticks.push(t);

  function show(index: number) {
    const row = items.current[index];
    const box = container.current;
    if (!row || !box) return;
    const bar = row.querySelector<HTMLElement>("[data-bar]");
    const boxRect = box.getBoundingClientRect();
    const barRect = (bar ?? row).getBoundingClientRect();
    const rowRect = row.getBoundingClientRect();
    const left = Math.min(Math.max(barRect.right - boxRect.left - 120, 8), boxRect.width - 248);
    const below = rowRect.top - boxRect.top < 150;
    setTipPos({ top: below ? rowRect.bottom - boxRect.top + 6 : rowRect.top - boxRect.top - 6, left: Math.max(left, 0), below });
    setActive(index);
  }

  function hide() {
    setActive(null);
    setTipPos(null);
  }

  function onKeyDown(event: KeyboardEvent<HTMLOListElement>) {
    const last = rows.length - 1;
    let next = focusIndex;
    if (event.key === "ArrowDown") next = Math.min(last, focusIndex + 1);
    else if (event.key === "ArrowUp") next = Math.max(0, focusIndex - 1);
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = last;
    else if (event.key === "Escape") return hide();
    else return;
    event.preventDefault();
    setFocusIndex(next);
    items.current[next]?.focus();
  }

  const activeRow = active === null ? null : rows[active];

  return (
    <div ref={container} className="bars" onPointerLeave={hide}>
      <ol className="bars-list" aria-label={caption} onKeyDown={onKeyDown}>
        {rows.map((row, i) => {
          const width = (row.value / top) * 100;
          return (
            <li
              key={row.id}
              ref={(el) => {
                items.current[i] = el;
              }}
              tabIndex={i === focusIndex ? 0 : -1}
              aria-describedby={active === i ? `${id}-tip` : undefined}
              className={`bars-row${row.highlight ? " is-highlight" : ""}${active === i ? " is-active" : ""}`}
              onPointerEnter={() => show(i)}
              onFocus={() => {
                setFocusIndex(i);
                show(i);
              }}
              onBlur={hide}
            >
              <span className="bars-label">
                <span className="bars-name">{row.label}</span>
                {row.note ? <span className="bars-note">{row.note}</span> : null}
              </span>
              <span className="bars-track">
                <span className="bars-plot">
                  <span data-bar className="bars-bar" style={{ width: `${Math.max(width, 0.4)}%` }} />
                  <span className="bars-value" style={{ left: `${Math.max(width, 0.4)}%` }}>
                    {row.display}
                  </span>
                </span>
              </span>
            </li>
          );
        })}
      </ol>
      <div className="bars-axis" aria-hidden>
        <span />
        <span className="bars-ticks">
          <span className="bars-plot">
            {ticks.map((t) => (
              <span key={t} className="bars-tick" style={{ left: `${(t / top) * 100}%` }}>
                {tick(t, format)}
              </span>
            ))}
          </span>
        </span>
      </div>
      <div className="bars-grid" aria-hidden>
        <span />
        <span className="bars-ticks">
          <span className="bars-plot">
            {ticks.map((t) => (
              <span key={t} className="bars-gridline" style={{ left: `${(t / top) * 100}%` }} />
            ))}
          </span>
        </span>
      </div>
      <div
        id={`${id}-tip`}
        role="tooltip"
        className={`bars-tip${activeRow && tipPos ? " is-visible" : ""}${tipPos?.below ? " is-below" : ""}`}
        style={tipPos ? { top: tipPos.top, left: tipPos.left } : undefined}
      >
        {activeRow ? (
          <>
            <p className="bars-tip-value">
              {activeRow.display}
              <span className="bars-tip-name">{activeRow.label}</span>
            </p>
            <dl>
              {activeRow.details.map(([label, value]) => (
                <div key={label}>
                  <dt>{label}</dt>
                  <dd>{value}</dd>
                </div>
              ))}
            </dl>
          </>
        ) : null}
      </div>
    </div>
  );
}
