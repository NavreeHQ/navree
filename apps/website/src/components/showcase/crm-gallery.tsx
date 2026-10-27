"use client";

import { useId, useRef, useState, type KeyboardEvent } from "react";

import { cn } from "@/lib/utils";

import { frontEnds, phoneScreens, screens, uiIds, type UiId } from "./crm-data";
import { BrowserFrame, PhoneFrame, ShotImage } from "./frames";

/** How wide the desktop shots are drawn: the full container, minus the bezel. */
const STAGE_SIZES = "(min-width: 1440px) 1360px, (min-width: 1024px) calc(100vw - 80px), (min-width: 640px) calc(100vw - 64px), calc(100vw - 48px)";
const PHONE_SIZES = "(min-width: 1024px) 280px, (min-width: 640px) 240px, calc(50vw - 36px)";

/** The Next.js / Nuxt switch: a two-button segmented control. */
export function UiToggle({ value, onChange, className }: { value: UiId; onChange: (ui: UiId) => void; className?: string }) {
  return (
    <div
      role="group"
      aria-label="Front end"
      className={cn("inline-flex h-10 items-center rounded-full border border-line bg-ink-1 p-1", className)}
    >
      {uiIds.map((id) => (
        <button
          key={id}
          type="button"
          aria-pressed={value === id}
          onClick={() => onChange(id)}
          className={cn(
            "inline-flex h-full flex-1 cursor-pointer items-center justify-center gap-2 rounded-full border px-4 text-sm font-medium whitespace-nowrap transition-[color,background-color,border-color] duration-200",
            value === id ? "border-line-2 bg-ink-3 text-fg" : "border-transparent text-fg-2 hover:text-fg",
          )}
        >
          <span
            aria-hidden
            className={cn("size-1.5 rounded-full transition-colors", value === id ? "bg-accent" : "bg-line-2")}
          />
          {frontEnds[id].name}
        </button>
      ))}
    </div>
  );
}

/**
 * The CRM gallery: every desktop screen in a browser frame, both phone
 * views, and a switch between the Next.js and the Nuxt front end. All the
 * pictures share one aspect ratio, so switching cross-fades in place and
 * nothing below moves.
 */
export function CrmGallery() {
  const [ui, setUi] = useState<UiId>("next");
  const [active, setActive] = useState(0);
  const id = useId();
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);
  const screen = screens[active];
  const frontEnd = frontEnds[ui];

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const last = screens.length - 1;
    let next = active;
    if (event.key === "ArrowRight") next = active === last ? 0 : active + 1;
    else if (event.key === "ArrowLeft") next = active === 0 ? last : active - 1;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = last;
    else return;
    event.preventDefault();
    setActive(next);
    tabs.current[next]?.focus();
    tabs.current[next]?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }

  return (
    <div>
      <div className="flex flex-col gap-4 lg:flex-row-reverse lg:items-end lg:justify-between lg:gap-8">
        <UiToggle value={ui} onChange={setUi} className="w-full sm:w-auto lg:mb-1.5" />
        <div className="relative -mx-4 min-w-0 sm:mx-0">
          <div
            role="tablist"
            aria-label="CRM screens"
            onKeyDown={onKeyDown}
            className="flex gap-1 overflow-x-auto px-4 [scrollbar-width:none] sm:px-0"
          >
            {screens.map((s, i) => (
              <button
                key={s.key}
                ref={(el) => {
                  tabs.current[i] = el;
                }}
                type="button"
                role="tab"
                id={`${id}-tab-${s.key}`}
                aria-selected={i === active}
                aria-controls={`${id}-panel`}
                tabIndex={i === active ? 0 : -1}
                onClick={() => setActive(i)}
                className={cn(
                  "relative flex h-12 shrink-0 cursor-pointer items-center gap-2 rounded-t-md px-3 text-[0.9375rem] font-medium whitespace-nowrap transition-colors duration-200 sm:px-4",
                  "after:absolute after:inset-x-2 after:-bottom-px after:h-px after:bg-accent after:opacity-0 after:shadow-glow-sm after:transition-opacity",
                  i === active ? "text-fg after:opacity-100" : "text-fg-3 hover:text-fg-1",
                )}
              >
                <span className={cn("font-mono text-xs numeric", i === active ? "text-accent" : "text-fg-3")}>
                  {String(i + 1).padStart(2, "0")}
                </span>
                {s.label}
              </button>
            ))}
          </div>
        </div>
      </div>
      <div aria-hidden className="h-px bg-line" />

      <div id={`${id}-panel`} role="tabpanel" aria-labelledby={`${id}-tab-${screen.key}`} className="mt-6 lg:mt-8">
        <BrowserFrame
          address={`${frontEnd.origin}${screen.path}`}
          bar={<span className="hidden font-mono text-xs text-fg-3 sm:inline">{frontEnd.app}</span>}
        >
          <div className="relative aspect-[16/10]">
            {uiIds.map((u) =>
              screens.map((s, i) => {
                const shown = u === ui && i === active;
                return (
                  <div
                    key={`${u}-${s.key}`}
                    aria-hidden={!shown}
                    className={cn(
                      "absolute inset-0 transition-opacity duration-300 ease-(--ease-out-quint) motion-reduce:transition-none",
                      shown ? "opacity-100" : "pointer-events-none opacity-0",
                    )}
                  >
                    <ShotImage
                      shot={frontEnds[u].shots[s.key]}
                      sizes={STAGE_SIZES}
                      priority={u === "next" && i === 0}
                      alt={shown ? undefined : ""}
                    />
                  </div>
                );
              }),
            )}
          </div>
        </BrowserFrame>

        <div className="mt-6 grid gap-x-12 gap-y-4 px-1 lg:grid-cols-12">
          <div className="lg:col-span-8">
            <h3 className="text-lg font-semibold text-fg">
              {screen.title}
              <span className="font-normal text-fg-3"> · {frontEnd.name}</span>
            </h3>
            <p className="mt-2 max-w-2xl text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{screen.text}</p>
          </div>
          <div className="lg:col-span-4 lg:justify-self-end lg:text-right">
            <p className="text-xs text-fg-3">Served by</p>
            <code className="mt-1.5 inline-block rounded-md border border-line bg-ink-1 px-2 py-1 font-mono text-xs break-all text-fg-1">
              {screen.api}
            </code>
          </div>
        </div>
      </div>

      <div className="mt-16 grid items-center gap-x-12 gap-y-10 border-t border-line pt-14 lg:mt-20 lg:grid-cols-12 lg:pt-16">
        <div className="lg:col-span-5">
          <h3 className="font-display text-display-md font-semibold text-balance text-fg">The same app on a phone.</h3>
          <p className="mt-5 max-w-xl text-[0.9375rem] leading-relaxed text-pretty text-fg-2 sm:text-lg">
            The sidebar becomes a sheet, the top bar keeps search, Create and notifications, and every screen works at
            390 px without scrolling sideways. Detail pages lead with a back arrow and the record&apos;s name.
          </p>
          <UiToggle value={ui} onChange={setUi} className="mt-8" />
        </div>
        <div className="grid grid-cols-2 gap-3 sm:gap-6 lg:col-span-7 lg:justify-items-center">
          {phoneScreens.map((p, i) => (
            <figure key={p.key} className={cn("w-full max-w-[17.5rem]", i === 1 && "lg:translate-y-10")}>
              <PhoneFrame>
                <div className="relative aspect-[390/844]">
                  {uiIds.map((u) => {
                    const shown = u === ui;
                    return (
                      <div
                        key={u}
                        aria-hidden={!shown}
                        className={cn(
                          "absolute inset-0 transition-opacity duration-300 ease-(--ease-out-quint) motion-reduce:transition-none",
                          shown ? "opacity-100" : "opacity-0",
                        )}
                      >
                        <ShotImage shot={frontEnds[u].shots[p.key]} sizes={PHONE_SIZES} alt={shown ? undefined : ""} />
                      </div>
                    );
                  })}
                </div>
              </PhoneFrame>
              <figcaption className="mt-3 text-center text-[0.8125rem] text-fg-3">
                {p.label} · {frontEnd.name}
              </figcaption>
            </figure>
          ))}
        </div>
      </div>
    </div>
  );
}
