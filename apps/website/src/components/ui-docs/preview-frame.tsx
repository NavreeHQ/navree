"use client";

import { useEffect, useRef, useState } from "react";

import { cn } from "@/lib/utils";

/**
 * A live preview: the static page `cargo run -p ui-docs` wrote for one
 * example, rendered by navree-ui with its own stylesheet and modules. The
 * frame keeps the toolkit's CSS and the site's apart, and carries the
 * toolkit's content security policy in its own document.
 *
 * The frame grows to fit what it shows (it is on this site's origin, so the
 * page can measure it); `height` is the least height, held before it loads.
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
  const [size, setSize] = useState(height);
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
      const pad = style ? Number.parseFloat(style.paddingTop) + Number.parseFloat(style.paddingBottom) : 0;
      setSize(Math.max(height, Math.ceil(stage.getBoundingClientRect().height + pad)));
    };
    const attach = () => {
      measure();
      const stage = frame.contentDocument?.querySelector("[data-preview-stage]");
      const Observer = (frame.contentWindow as (Window & typeof globalThis) | null)?.ResizeObserver;
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
    };
  }, [height, fit]);

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
          ? { height: phoneHeight }
          : width
            ? { width, height: size, transform: `scale(${scale})`, transformOrigin: "0 0" }
            : { height: thumb ? "100%" : size }
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
        style={{ height: phoneMode ? phoneHeight : thumb ? "100%" : Math.ceil(size * scale) }}
      >
        {frame}
      </div>
      {fallback}
    </>
  );
}
