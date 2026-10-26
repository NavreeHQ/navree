"use client";

import { Children, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

/**
 * The interactive part of `<Tabs>` and `<CodeGroup>`: a tab list and one
 * panel per child. The labels are read on the server, so this component
 * only shows and hides panels (WAI-ARIA tabs pattern, arrow keys move).
 */
export function TabsShell({
  labels,
  children,
  variant = "tabs",
}: {
  labels: string[];
  children: ReactNode;
  variant?: "tabs" | "code";
}) {
  const [active, setActive] = useState(0);
  const id = useId();
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);
  const panels = Children.toArray(children);

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const last = labels.length - 1;
    let next = active;
    if (event.key === "ArrowRight") next = active === last ? 0 : active + 1;
    else if (event.key === "ArrowLeft") next = active === 0 ? last : active - 1;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = last;
    else return;
    event.preventDefault();
    setActive(next);
    tabs.current[next]?.focus();
  }

  return (
    <div className={variant === "code" ? "tabs tabs-code" : "tabs"}>
      <div role="tablist" aria-orientation="horizontal" className="tabs-list" onKeyDown={onKeyDown}>
        {labels.map((label, i) => (
          <button
            key={`${label}-${i}`}
            ref={(el) => {
              tabs.current[i] = el;
            }}
            type="button"
            role="tab"
            id={`${id}-tab-${i}`}
            aria-selected={i === active}
            aria-controls={`${id}-panel-${i}`}
            tabIndex={i === active ? 0 : -1}
            onClick={() => setActive(i)}
            className="tabs-tab"
          >
            {label}
          </button>
        ))}
      </div>
      {panels.map((panel, i) => (
        <div
          key={i}
          role="tabpanel"
          id={`${id}-panel-${i}`}
          aria-labelledby={`${id}-tab-${i}`}
          hidden={i !== active}
          className="tabs-panel"
        >
          {panel}
        </div>
      ))}
    </div>
  );
}
