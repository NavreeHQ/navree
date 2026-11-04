"use client";

import { useEffect, useState, type ReactNode } from "react";

import { PreviewFrame } from "./preview-frame";

/** Where the demo says it is up: `204` from the demo app, anything else when it isn't there. */
const HEALTH = "/demo/ui/health";
/** How long the demo has to answer before the picture stays. */
const PATIENCE_MS = 3000;
/** The live frame's height: an app window, which scrolls inside. */
const LIVE_HEIGHT = 760;

/** Whether the frame shows one of the demo's own pages: under `/demo/ui/`, drawn by the toolkit. */
function showsDemo(frame: HTMLIFrameElement): boolean {
  try {
    const path = frame.contentWindow?.location.pathname ?? "";
    const doc = frame.contentDocument;
    return path.startsWith("/demo/ui/") && !!doc?.documentElement.hasAttribute("data-build");
  } catch {
    return false;
  }
}

/**
 * A preview with a live app behind it, when that app answers: the theme builder,
 * served by the demo app at `/demo/ui/`, where every control works. Until the demo
 * says it is up, and whenever it isn't (it doesn't answer, or a page in the frame
 * isn't one of its own: an error from the proxy, say), the frame shows `picture`,
 * the static preview, which says what each control would ask a server for. The
 * live frame loads only after the demo has answered, so a reader never sees a
 * page that isn't there.
 */
export function LiveDemo({ live, title, picture }: { live: string; title: string; picture: ReactNode }) {
  const [state, setState] = useState<"asking" | "live" | "picture">("asking");

  useEffect(() => {
    const stop = new AbortController();
    const patience = setTimeout(() => stop.abort(), PATIENCE_MS);
    fetch(HEALTH, { cache: "no-store", signal: stop.signal })
      .then((response) => setState(response.ok ? "live" : "picture"))
      .catch(() => setState("picture"))
      .finally(() => clearTimeout(patience));
    return () => {
      clearTimeout(patience);
      stop.abort();
    };
  }, []);

  if (state !== "live") {
    return (
      <div data-live-demo={state}>
        {picture}
        {state === "picture" ? (
          <p className="border-t border-line px-4 py-2.5 text-xs leading-relaxed text-fg-3">
            A picture of the builder: the live demo isn&apos;t answering, so each control says what your app would
            ask its server for.
          </p>
        ) : null}
      </div>
    );
  }
  return (
    <div data-live-demo="live">
      <PreviewFrame
        src={live}
        title={title}
        height={LIVE_HEIGHT}
        fit={false}
        forms
        onLoad={(frame) => {
          if (!showsDemo(frame)) setState("picture");
        }}
      />
      <p className="border-t border-line px-4 py-2.5 text-xs leading-relaxed text-fg-3">
        Live: a small Navree app behind this site draws the builder, and every change goes to its server. It keeps
        nothing: a draft lives in its address.
      </p>
    </div>
  );
}
