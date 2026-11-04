"use client";

import { Pause, Play } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import type { LiveScene } from "./scene/scene";
import { BEATS } from "./scene/timeline";

/**
 * The live about picture: the still picture comes alive as a three.js scene
 * (`scene/`), built from the very image the page already shows. Everything
 * about when it runs lives here.
 *
 * It runs only where it can run well, and the still picture stays
 * otherwise: never under `prefers-reduced-motion`, without WebGL 2 (or on a
 * software renderer), on devices with under 4 GB of memory or two cores, or
 * with Save-Data on. three.js is fetched only then, when the picture comes
 * near the screen and the browser is idle, after the page has painted.
 *
 * While it runs:
 * - the opening (digits raining down and gathering into the family) plays
 *   once per visit; a click on the picture skips it;
 * - it stops completely off screen and in a hidden tab;
 * - the camera follows the pointer and the scroll a little;
 * - it draws at most 60 frames a second (30 on phones and small CPUs), at
 *   most 1.5 device pixels per CSS pixel, and draws less when frames come
 *   late;
 * - the button pauses and plays it; a paused visitor stays paused on the
 *   next visit (and the scene isn't even loaded until they press play).
 *
 * Nothing is read from the layout per frame: sizes are read when the
 * picture is resized, and the scroll position from `window.scrollY`.
 */

const PAUSED_KEY = "navree:about-motion";
const SEEN_KEY = "navree:about-opening";
const MAX_RATIO = 1.5;
/** How far the pointer and the scroll move the camera, in the picture's pixels. */
const SWAY = { x: 28, y: 16, scroll: 14 } as const;

type Nav = Navigator & { deviceMemory?: number; connection?: { saveData?: boolean } };
type CaptureWindow = Window & {
  __navreeAboutCapture?: boolean;
  __navreeAboutScene?: { ready: boolean; render(t: number, view?: { x: number; y: number }): void; glyphs: number };
};

const storage = {
  get(store: "local" | "session", key: string): string | null {
    try {
      return (store === "local" ? localStorage : sessionStorage).getItem(key);
    } catch {
      return null;
    }
  },
  set(store: "local" | "session", key: string, value: string | null) {
    try {
      const s = store === "local" ? localStorage : sessionStorage;
      if (value === null) s.removeItem(key);
      else s.setItem(key, value);
    } catch {
      // A private window or blocked storage: the choice lasts this page only.
    }
  },
};

/** Whether this device should get the live picture at all. */
function capable(): boolean {
  const nav = navigator as Nav;
  if ((nav.deviceMemory ?? 8) < 4 || nav.connection?.saveData || (nav.hardwareConcurrency ?? 4) <= 2) return false;
  try {
    const probe = document.createElement("canvas");
    const gl = probe.getContext("webgl2", { failIfMajorPerformanceCaveat: true });
    if (!gl) return false;
    gl.getExtension("WEBGL_lose_context")?.loseContext();
    return true;
  } catch {
    return false;
  }
}

const idle = (fn: () => void) => {
  if (typeof window.requestIdleCallback === "function") window.requestIdleCallback(fn, { timeout: 1500 });
  else setTimeout(fn, 200);
};

/**
 * The canvas over the still picture, and the pause button. It goes inside
 * the picture's wrapper, next to the still's frame; `frameClass` gives the
 * canvas the frame's own size and offset.
 */
export function FamilyLive({ frameClass }: { frameClass: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const [control, setControl] = useState<"none" | "pause" | "play">("none");
  const api = useRef<{ toggle(): void } | null>(null);

  useEffect(() => {
    const canvas = ref.current;
    const wrapper = canvas?.parentElement?.parentElement;
    if (!canvas || !wrapper) return;
    const capture = Boolean((window as CaptureWindow).__navreeAboutCapture);
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    if (!capture && (motion.matches || !capable())) return;

    let scene: LiveScene | null = null;
    let loading = false;
    let dead = false;
    let onScreen = false;
    let paused = !capture && storage.get("local", PAUSED_KEY) === "paused";
    let size = { width: 0, height: 0, top: 0 };
    // The clock: `t` advances with real time while running; `rate` above 1 fast-forwards the opening.
    let t = storage.get("session", SEEN_KEY) ? BEATS.settled : 0;
    let rate = 1;
    let raf = 0;
    let last = 0;
    let lastDraw = 0;
    const view = { x: 0, y: 0 };
    const target = { x: 0, y: 0 };
    let seen = t >= BEATS.settled;
    // An opening interrupted (scrolled away, tab hidden) resumes as the finished picture.
    let interrupted = false;
    // Frame budget, and stepping down when frames come late.
    const coarse = window.matchMedia("(pointer: coarse)").matches || (navigator.hardwareConcurrency ?? 4) < 6;
    let interval = 1000 / (coarse ? 30 : 60);
    let ratio = Math.min(window.devicePixelRatio || 1, MAX_RATIO);
    let step = 0;
    let late = 0;
    let frames = 0;

    const measure = () => {
      const rect = wrapper.getBoundingClientRect();
      const box = canvas.getBoundingClientRect();
      size = { width: Math.round(box.width), height: Math.round(box.height), top: rect.top + window.scrollY };
    };

    const draw = () => scene?.render(t, view);

    const frame = (now: number) => {
      raf = 0;
      if (!scene || !running()) return;
      raf = requestAnimationFrame(frame);
      if (now - lastDraw < interval - 2) return;
      const dt = last ? Math.min(0.1, (now - last) / 1000) : 0;
      // Late frames: after a second of them, draw less (pixels first, then glyphs, then frames).
      if (last && now - last > interval * 1.6) late++;
      if (++frames % 60 === 0) {
        if (late > 20) degrade();
        late = 0;
      }
      last = now;
      lastDraw = now;
      t += dt * rate;
      if (rate > 1 && t >= BEATS.settled) rate = 1;
      if (!seen && t >= BEATS.settled) {
        seen = true;
        storage.set("session", SEEN_KEY, "1");
      }
      const k = 1 - Math.exp(-dt * 5);
      const scrollY = ((size.top + size.height / 2 - window.scrollY - window.innerHeight / 2) / window.innerHeight) * SWAY.scroll;
      view.x += (target.x - view.x) * k;
      view.y += (target.y + scrollY - view.y) * k;
      draw();
    };

    const degrade = () => {
      if (!scene) return;
      if (ratio > 1) {
        ratio = Math.max(1, ratio - 0.25);
        scene.resize(size.width, size.height, ratio);
      } else if (scene.setStep(step + 1)) {
        step++;
      } else if (interval < 30) {
        interval = 1000 / 30;
      }
    };

    const running = () => !dead && !paused && onScreen && document.visibilityState === "visible";

    const sync = () => {
      if (!scene) {
        if (!paused && onScreen && !loading) load();
        return;
      }
      if (running()) {
        if (!raf) {
          if (interrupted && t < BEATS.settled) {
            t = BEATS.settled;
            rate = 1;
            draw();
          }
          interrupted = false;
          last = 0;
          raf = requestAnimationFrame(frame);
        }
      } else if (raf) {
        cancelAnimationFrame(raf);
        raf = 0;
        if (!paused) interrupted = true;
      }
    };

    const load = () => {
      loading = true;
      idle(async () => {
        try {
          if (dead) return;
          await document.fonts.ready;
          const image = wrapper.querySelector<HTMLImageElement>("img[data-about-picture]");
          if (!image) return;
          if (!image.complete || !image.naturalWidth) await image.decode();
          measure();
          const font = getComputedStyle(canvas).fontFamily;
          await document.fonts.load(`32px ${font}`, "01{}()#[]").catch(() => []);
          const { createScene, TIERS } = await import("./scene/scene");
          if (dead) return;
          const built = await createScene({
            canvas,
            image,
            tier: size.width < 640 ? TIERS.narrow : TIERS.wide,
            font,
            width: size.width,
            height: size.height,
            pixelRatio: ratio,
          });
          if (dead) {
            built?.dispose();
            return;
          }
          if (!built) return;
          scene = built;
          draw();
          wrapper.dataset.live = "";
          setControl("pause");
          if (capture) {
            (window as CaptureWindow).__navreeAboutScene = {
              ready: true,
              render: (time: number, at?: { x: number; y: number }) => built.render(time, at),
              glyphs: built.glyphs,
            };
            return;
          }
          sync();
        } catch {
          // The still picture stays.
        } finally {
          loading = false;
        }
      });
    };

    const stopForGood = () => {
      dead = true;
      if (raf) cancelAnimationFrame(raf);
      raf = 0;
      scene?.dispose();
      scene = null;
      delete wrapper.dataset.live;
    };

    api.current = {
      toggle() {
        paused = !paused;
        storage.set("local", PAUSED_KEY, paused ? "paused" : null);
        setControl(paused ? "play" : "pause");
        // Pausing during the opening shows the finished picture, never digits in mid-air.
        if (paused && t < BEATS.settled) {
          t = BEATS.settled;
          draw();
        }
        sync();
      },
    };
    if (paused) setControl("play");

    const skip = () => {
      if (scene && t < BEATS.settled - 0.4) rate = 6;
    };
    const onPointer = (e: PointerEvent) => {
      if (e.pointerType !== "mouse") return;
      const cy = size.top + size.height / 2 - window.scrollY;
      target.x = Math.max(-1, Math.min(1, (e.clientX / window.innerWidth) * 2 - 1)) * SWAY.x;
      target.y = Math.max(-1, Math.min(1, -(e.clientY - cy) / (window.innerHeight / 2))) * SWAY.y;
    };
    const onResize = () => {
      measure();
      if (scene && size.width > 0) {
        scene.resize(size.width, size.height, ratio);
        draw();
      }
    };
    const onMotion = () => {
      if (motion.matches) stopForGood();
    };

    const io = new IntersectionObserver(
      ([entry]) => {
        onScreen = Boolean(entry?.isIntersecting);
        sync();
      },
      { rootMargin: "160px 0px" },
    );
    const ro = new ResizeObserver(onResize);
    io.observe(wrapper);
    ro.observe(wrapper);
    document.addEventListener("visibilitychange", sync);
    motion.addEventListener("change", onMotion);
    window.addEventListener("pointermove", onPointer, { passive: true });
    wrapper.addEventListener("click", skip);
    if (capture) {
      onScreen = true;
      measure();
      load();
    }
    return () => {
      io.disconnect();
      ro.disconnect();
      document.removeEventListener("visibilitychange", sync);
      motion.removeEventListener("change", onMotion);
      window.removeEventListener("pointermove", onPointer);
      wrapper.removeEventListener("click", skip);
      stopForGood();
      api.current = null;
    };
  }, []);

  return (
    <>
      <div aria-hidden className={`pointer-events-none absolute inset-y-0 left-0 font-mono ${frameClass}`}>
        <canvas
          ref={ref}
          width={0}
          height={0}
          className="block size-full opacity-0 transition-opacity duration-700 ease-(--ease-out-quint) group-data-live/picture:opacity-100"
        />
      </div>
      {control !== "none" ? (
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            api.current?.toggle();
          }}
          aria-label={control === "pause" ? "Pause the animation" : "Play the animation"}
          className="absolute right-3 bottom-3 inline-flex size-8 items-center justify-center rounded-full border border-line-2 bg-ink-1/80 text-fg-2 transition-colors hover:bg-ink-2 hover:text-fg max-sm:right-4"
        >
          {control === "pause" ? <Pause className="size-3.5" aria-hidden /> : <Play className="size-3.5" aria-hidden />}
        </button>
      ) : null}
    </>
  );
}
