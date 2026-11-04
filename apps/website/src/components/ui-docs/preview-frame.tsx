"use client";

import { useEffect, useRef, useState } from "react";

import { cn } from "@/lib/utils";

/** What opens over a preview: a popover, menu, list or picker, or a modal dialog. */
const OPEN = ":popover-open, dialog:modal";
/**
 * What is open and never needs room: the preview's note, which says what a form
 * would send, and a toaster, shown from the start and drawn in place.
 */
const PASSED_OVER = "[data-preview-note], [data-slot=toaster]";
/** The toolkit's lists are at most 22rem tall: a list with every option out is no taller. */
const LIST_MOST = 352;
/** A list that does not fit this cap at full length: the select popups and the country list. */
const LISTS = "[data-ui-fit], [data-slot=phone-countries]";
/** The gap between a popup and its opener, as the toolkit places it. */
const GAP = 4;
/** The room kept under a popup, a little more than the toolkit's 8 px margin. */
const MARGIN = 12;
/** The tallest a frame grows for what opens in it. */
const ROOM_MOST = 1200;
/** How long a frame keeps its room after the last popup closes: one that opens again at once finds it there. */
const SETTLE_MS = 150;

type FrameWindow = Window & typeof globalThis;

/** The popups and dialogs open in `doc`, less those that never need room. */
function openIn(doc: Document): HTMLElement[] {
  try {
    return [...doc.querySelectorAll<HTMLElement>(OPEN)].filter((el) => !el.matches(PASSED_OVER));
  } catch {
    // An engine without `:popover-open` or `:modal` has none of them open.
    return [];
  }
}

/** What opens `pop`, found the way the toolkit finds it to place the popup. */
function openerOf(pop: HTMLElement, win: FrameWindow): Element | null {
  if (!pop.id) return null;
  const id = win.CSS.escape(pop.id);
  return pop.ownerDocument.querySelector(`[popovertarget="${id}"],[data-ui-hover="${id}"],[data-ui-anchor="${id}"]`);
}

/**
 * How tall the frame must be for `pop` to show whole, in the frame's own px: its
 * height as it would be with room (not fitted to the frame), under or beside
 * its opener, or in the middle of the frame for a dialog.
 */
function roomFor(pop: HTMLElement, win: FrameWindow): number {
  const kept = pop.style.maxHeight;
  pop.style.maxHeight = "none";
  // The layout height: a popup fading in is drawn a little scaled.
  let height = pop.offsetHeight;
  pop.style.maxHeight = kept;
  if (pop.matches(LISTS)) height = Math.min(height, LIST_MOST);
  const opener = openerOf(pop, win);
  if (opener) {
    const at = opener.getBoundingClientRect();
    const named = pop.getAttribute("data-side") || "bottom";
    if (named === "top" || named === "bottom") {
      // A popup that asks for the top and fits there needs nothing more.
      if (named === "top" && at.top >= height + GAP + MARGIN) return 0;
      return at.bottom + GAP + height + MARGIN;
    }
    // Beside its opener, lined up with it.
    return Math.max(at.bottom, at.top + height) + MARGIN;
  }
  if (!(pop instanceof win.HTMLDialogElement)) return 0;
  const slot = pop.getAttribute("data-slot");
  const side = pop.getAttribute("data-side");
  // A sheet at the left or right is as tall as any frame.
  if (slot === "sheet" && (side === "left" || side === "right")) return 0;
  if (slot === "sheet") return height / 0.8;
  if (slot === "drawer") return height / 0.85;
  if (slot === "command-dialog") return Math.max(height + 64, height / 0.88);
  return height + 32;
}

/** A preview watched for popups: `stop` ends it, `release` lets the body follow the frame again. */
type Watch = { stop: () => void; release: () => void };

/** No preview watched yet. */
const IDLE: Watch = { stop: () => {}, release: () => {} };

/**
 * Watches the preview in `frame` for popups and dialogs. When one opens that the
 * frame is too short for, `grow` gets the height that shows it whole; `settle`
 * is called once the last one has been closed for a moment. While the frame is
 * taller than before, the preview's body keeps its height, so nothing in it
 * moves: a centred control stays under the pointer. Once the frame is back at
 * its height, `release` lets the body follow it again.
 */
function watchOverlays(frame: HTMLIFrameElement, grow: (height: number) => void, settle: () => void): Watch {
  const doc = frame.contentDocument;
  const win = frame.contentWindow as FrameWindow | null;
  if (!doc?.body || !win) return IDLE;
  const body = doc.body;
  let pending = 0;
  let quiet = 0;
  let held = false;
  const watched = new Set<Element>();
  const sizes = new win.ResizeObserver(() => schedule());

  const update = () => {
    pending = 0;
    const shown = openIn(doc);
    for (const el of watched) {
      if (!shown.includes(el)) {
        watched.delete(el);
        sizes.unobserve(el);
      }
    }
    if (!shown.length) {
      if (!quiet)
        quiet = win.setTimeout(() => {
          quiet = 0;
          if (openIn(doc).length) return;
          held = false;
          settle();
        }, SETTLE_MS);
      return;
    }
    win.clearTimeout(quiet);
    quiet = 0;
    let want = 0;
    for (const pop of shown) {
      if (!watched.has(pop)) {
        watched.add(pop);
        sizes.observe(pop);
      }
      want = Math.max(want, roomFor(pop, win));
    }
    want = Math.min(ROOM_MOST, Math.ceil(want));
    if (want <= win.innerHeight) return;
    if (!held) {
      held = true;
      body.style.minHeight = `${win.innerHeight}px`;
    }
    grow(want);
  };
  const schedule = () => {
    if (!pending) pending = win.requestAnimationFrame(update);
  };

  // A popover says so with `toggle`; a list placed again as it filters, with `ui:place`;
  // a modal dialog, with its `open` attribute.
  doc.addEventListener("toggle", schedule, true);
  doc.addEventListener("ui:place", schedule, true);
  const dialogs = new win.MutationObserver(schedule);
  dialogs.observe(body, { subtree: true, attributes: true, attributeFilter: ["open"] });
  return {
    stop: () => {
      doc.removeEventListener("toggle", schedule, true);
      doc.removeEventListener("ui:place", schedule, true);
      dialogs.disconnect();
      sizes.disconnect();
      win.cancelAnimationFrame(pending);
      win.clearTimeout(quiet);
      body.style.minHeight = "";
    },
    release: () => {
      if (!held) body.style.minHeight = "";
    },
  };
}

/**
 * A live preview: the static page `cargo run -p ui-docs` wrote for one
 * example, rendered by navree-ui with its own stylesheet and modules. The
 * frame keeps the toolkit's CSS and the site's apart, and carries the
 * toolkit's content security policy in its own document.
 *
 * The frame grows to fit what it shows (it is on this site's origin, so the
 * page can measure it); `height` is the least height, held before it loads.
 * A popup, list, picker or dialog that opens in it is in the top layer and
 * takes no room in the page, so the frame also grows while one is open, to
 * show it whole, and goes back once it closes; while it is open the frame
 * never shrinks, so nothing moves under the pointer.
 *
 * With `width`, the page is drawn that wide and scaled down to the column,
 * so a whole page keeps its wide layout. With `phoneHeight` too, a column
 * narrower than half of `width` gets the page at its own width and that tall,
 * as a phone shows it, rather than a picture too small to read.
 *
 * With `defer`, the frame asks for its page only once the page around it has
 * loaded and the frame is about to scroll into view, so the frames of a long
 * page never compete with its first paint. Without JavaScript a `<noscript>`
 * frame shows the page instead.
 */
export function PreviewFrame({
  src,
  title,
  height,
  width,
  fit = true,
  thumb = false,
  phoneHeight,
  defer = false,
  className,
}: {
  src: string;
  title: string;
  height: number;
  width?: number | null;
  /** Grow to the content. Off for whole pages, which fill their frame. */
  fit?: boolean;
  /** A picture on the landing grid: not focusable, not clickable. */
  thumb?: boolean;
  /** For a whole page: on a column narrower than half of `width`, draw it at the column's width, this tall. */
  phoneHeight?: number;
  /** Load only after the page has loaded, as the frame nears the screen. */
  defer?: boolean;
  className?: string;
}) {
  const ref = useRef<HTMLIFrameElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const remeasure = useRef<() => void>(() => {});
  const overlays = useRef<Watch>(IDLE);
  const grown = useRef(false);
  const [size, setSize] = useState(height);
  const [room, setRoom] = useState(0);
  const [scale, setScale] = useState(1);
  const [phone, setPhone] = useState(false);
  const [near, setNear] = useState(!defer);

  useEffect(() => {
    if (near) return;
    let observer: IntersectionObserver | null = null;
    const watch = () => {
      const frame = box.current ?? ref.current;
      if (!frame) return;
      observer = new IntersectionObserver(
        (entries) => {
          if (entries.some((entry) => entry.isIntersecting)) {
            setNear(true);
            observer?.disconnect();
          }
        },
        { rootMargin: "200px 0px" },
      );
      observer.observe(frame);
    };
    if (document.readyState === "complete") watch();
    else addEventListener("load", watch, { once: true });
    return () => {
      removeEventListener("load", watch);
      observer?.disconnect();
    };
  }, [near]);
  const address = near ? src : undefined;
  const fallback = defer ? (
    <noscript>
      <iframe src={src} title={title} loading="lazy" className="block w-full border-0 bg-ink-0" style={{ height }} />
    </noscript>
  ) : null;

  useEffect(() => {
    const frame = ref.current;
    if (!frame || !fit) return;
    let observer: ResizeObserver | null = null;
    const measure = () => {
      const doc = frame.contentDocument;
      const stage = doc?.querySelector("[data-preview-stage]");
      if (!doc?.body || !stage) return;
      const style = doc.defaultView?.getComputedStyle(doc.body);
      // While something is open over the preview the frame keeps its height: a picture
      // drawn in place that opens for real leaves the page, and the page must not jump.
      // It is measured again once everything is closed and the frame is back at its height.
      if (openIn(doc).length) return;
      const pad = style ? Number.parseFloat(style.paddingTop) + Number.parseFloat(style.paddingBottom) : 0;
      setSize(Math.max(height, Math.ceil(stage.getBoundingClientRect().height + pad)));
    };
    remeasure.current = measure;
    const attach = () => {
      measure();
      const stage = frame.contentDocument?.querySelector("[data-preview-stage]");
      const Observer = (frame.contentWindow as FrameWindow | null)?.ResizeObserver;
      if (stage && Observer) {
        observer?.disconnect();
        observer = new Observer(measure);
        observer.observe(stage);
      }
    };
    frame.addEventListener("load", attach);
    if (frame.contentDocument?.querySelector("[data-preview-stage]")) attach();
    return () => {
      frame.removeEventListener("load", attach);
      observer?.disconnect();
      remeasure.current = () => {};
    };
  }, [height, fit]);

  useEffect(() => {
    const frame = ref.current;
    if (!frame || thumb) return;
    const attach = () => {
      overlays.current.stop();
      setRoom(0);
      overlays.current = watchOverlays(
        frame,
        (want) => setRoom((current) => Math.max(current, want)),
        // A frame that grew is measured once it is back at its height (below); one that
        // didn't, now: what it shows may have changed while a popup was open.
        () => (grown.current ? setRoom(0) : remeasure.current()),
      );
    };
    frame.addEventListener("load", attach);
    if (frame.contentDocument?.querySelector("[data-preview-stage]")) attach();
    return () => {
      frame.removeEventListener("load", attach);
      overlays.current.stop();
      overlays.current = IDLE;
    };
  }, [thumb]);

  // The frame is back at its height, drawn: the preview's body may follow it again,
  // and what it shows is measured afresh.
  useEffect(() => {
    grown.current = room > 0;
    if (room) return;
    overlays.current.release();
    remeasure.current();
  }, [room]);

  useEffect(() => {
    const outer = box.current;
    if (!outer || !width) return;
    const update = () => {
      setScale(Math.min(1, outer.clientWidth / width));
      setPhone(!!phoneHeight && outer.clientWidth < width / 2);
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(outer);
    return () => observer.disconnect();
  }, [width, phoneHeight]);

  // One wrapper and one frame in every mode, so the observers above keep watching them.
  const phoneMode = !!(width && phone && phoneHeight);
  const tall = Math.max(size, room);
  const phoneTall = Math.max(phoneHeight ?? 0, room);
  const frame = (
    <iframe
      ref={ref}
      src={address}
      data-deferred={defer || undefined}
      title={title}
      loading="lazy"
      sandbox="allow-scripts allow-same-origin"
      {...(thumb ? { "aria-hidden": true, tabIndex: -1 } : {})}
      style={
        phoneMode
          ? { height: phoneTall }
          : width
            ? { width, height: tall, transform: `scale(${scale})`, transformOrigin: "0 0" }
            : { height: thumb ? "100%" : tall }
      }
      className={cn(
        "block border-0 bg-ink-0",
        (!width || phoneMode) && "w-full",
        thumb && "pointer-events-none",
        className,
      )}
    />
  );
  if (!width) {
    return fallback ? (
      <>
        {frame}
        {fallback}
      </>
    ) : (
      frame
    );
  }
  return (
    <>
      <div
        ref={box}
        data-deferred={defer || undefined}
        className="w-full overflow-hidden"
        style={{ height: phoneMode ? phoneTall : thumb ? "100%" : Math.ceil(tall * scale) }}
      >
        {frame}
      </div>
      {fallback}
    </>
  );
}
