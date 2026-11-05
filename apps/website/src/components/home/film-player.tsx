"use client";

import { LoaderCircleIcon, PlayIcon } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { cn } from "@/lib/utils";

export type FilmSource = { readonly src: string; readonly type: string };

type Phase = "idle" | "loading" | "playing" | "failed";

/**
 * The button and the loading note are one dark glass capsule in a corner of the poster (the top
 * left on a narrow phone, where the bottom left would cover the poster's address), kept small so
 * it clears the poster's own text at every width the hero draws it.
 */
const CORNER = "absolute inset-0 flex items-end p-3 max-[30rem]:items-start";
const PILL =
  "inline-flex items-center gap-2.5 rounded-full border border-white/15 bg-black/70 py-1 pr-3.5 pl-1 text-[0.8125rem] font-medium text-white backdrop-blur-md sm:gap-3 sm:pr-4 sm:text-sm";
const DISC = "inline-flex size-8 items-center justify-center rounded-full sm:size-9";

/**
 * The launch film on the hero: the poster, a play button and its length, and
 * nothing of the film downloaded until the button is pressed.
 *
 * - The poster is a plain `<img>` with its size, so the first paint has it and
 *   the page does not move. The `<video>` has `preload="none"` and no source
 *   until the press, so a visitor who never presses play downloads none of it.
 * - A press picks the first file the browser can decode (the AV1 WebM, then
 *   the H.264 MP4; a phone, or a visitor who asked to save data, gets the
 *   720p MP4 first), starts it from 0:00 with sound inside the page, and
 *   shows the native controls once it plays. If a file fails it tries the
 *   next. When the film ends, the poster comes back and the next press plays
 *   it from the start again, as does a return to the page through the
 *   browser's back button.
 * - Without JavaScript a `<noscript>` video with native controls takes its
 *   place.
 *
 * The press, the choice of file and `play()` happen in one go inside the click:
 * Safari only lets a film start with sound when `play()` runs in the gesture.
 */
export function FilmPlayer({
  title,
  poster,
  sources,
  phoneSources,
  file,
  length,
  spokenLength,
  describedBy,
}: {
  title: string;
  poster: { webp: string; jpg: string; width: number; height: number; alt: string };
  sources: readonly FilmSource[];
  phoneSources: readonly FilmSource[];
  /** The file to link to when none of the others plays. */
  file: string;
  /** "1:05". */
  length: string;
  /** "1 minute 5 seconds". */
  spokenLength: string;
  /** The id of the text that says what the film shows. */
  describedBy: string;
}) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const tried = useRef<{ list: readonly FilmSource[]; index: number } | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  const [watched, setWatched] = useState(false);

  /** Gives the video the first file of `list`, from `from` on, that this browser says it can play. */
  const choose = useCallback((list: readonly FilmSource[], from: number): boolean => {
    const video = videoRef.current;
    if (!video) return false;
    for (let i = from; i < list.length; i++) {
      if (video.canPlayType(list[i].type) !== "") {
        tried.current = { list, index: i };
        video.src = list[i].src;
        return true;
      }
    }
    return false;
  }, []);

  /**
   * A play() the browser refuses (a policy that blocks sound) leaves the film loaded with its
   * controls showing, so the visitor can press play there. Any other failure is a file that
   * cannot be loaded or decoded, which `next` handles from the video's error event.
   */
  const play = (video: HTMLVideoElement) => {
    video.play().catch((error: unknown) => {
      if (error instanceof DOMException && error.name === "NotAllowedError") setPhase((now) => (now === "loading" ? "playing" : now));
    });
  };

  const start = () => {
    const video = videoRef.current;
    if (!video) return;
    const connection = (navigator as Navigator & { connection?: { saveData?: boolean } }).connection;
    const small = connection?.saveData === true || window.matchMedia("(max-width: 47.99rem), (pointer: coarse) and (max-height: 32rem)").matches;
    const list = small ? phoneSources : sources;
    if (tried.current?.list !== list && !choose(list, 0)) {
      setPhase("failed");
      return;
    }
    video.currentTime = 0;
    setPhase("loading");
    play(video);
  };

  /** A file that cannot be loaded or decoded: the next one, or the link. A film that broke while playing is not started again. */
  const next = () => {
    const video = videoRef.current;
    const was = tried.current;
    if (!video || !was) return;
    if (video.readyState === 0 && choose(was.list, was.index + 1)) play(video);
    else setPhase("failed");
  };

  // The film starts from the start when the page comes back from the browser's cache.
  useEffect(() => {
    const back = (event: PageTransitionEvent) => {
      if (!event.persisted) return;
      videoRef.current?.pause();
      if (videoRef.current) videoRef.current.currentTime = 0;
      setPhase("idle");
    };
    window.addEventListener("pageshow", back);
    return () => window.removeEventListener("pageshow", back);
  }, []);

  // The button that was pressed leaves with the poster, so the keyboard moves on to the film's own
  // controls, and back to the button when the film ends and the poster returns.
  useEffect(() => {
    if (phase === "playing") videoRef.current?.focus({ preventScroll: true });
    else if (phase === "idle" && watched) buttonRef.current?.focus({ preventScroll: true });
  }, [phase, watched]);

  const showPoster = phase !== "playing";

  return (
    <div className="bezel-core relative aspect-video bg-black">
      <div className="film-js">
        <picture className={cn(!showPoster && "hidden")}>
          <source type="image/webp" srcSet={poster.webp} />
          <img
            src={poster.jpg}
            alt={poster.alt}
            width={poster.width}
            height={poster.height}
            fetchPriority="high"
            decoding="async"
            className="absolute inset-0 size-full object-cover"
          />
        </picture>
        <video
          ref={videoRef}
          preload="none"
          playsInline
          controls={phase === "playing"}
          aria-label={title}
          aria-describedby={describedBy}
          onPlaying={() => setPhase("playing")}
          onEnded={() => {
            setWatched(true);
            setPhase("idle");
          }}
          onError={next}
          className={cn(
            "absolute inset-0 size-full bg-black object-contain outline-offset-[-4px]",
            phase === "idle" || phase === "failed" ? "hidden" : phase === "loading" ? "opacity-0" : null,
          )}
        />

        {phase === "idle" ? (
          <button
            ref={buttonRef}
            type="button"
            onClick={start}
            aria-label={`${watched ? "Play again: the" : "Play the"} launch film, ${spokenLength}`}
            className={cn(CORNER, "group cursor-pointer text-left outline-offset-[-4px]")}
          >
            <span className={cn(PILL, "transition-colors group-hover:border-accent/60 group-hover:bg-black/85")}>
              <span
                aria-hidden
                className={cn(
                  DISC,
                  "bg-accent text-accent-ink shadow-glow transition-transform duration-300 ease-(--ease-out-quint) group-hover:scale-105 motion-reduce:transition-none motion-reduce:group-hover:scale-100",
                )}
              >
                <PlayIcon className="size-4 translate-x-px fill-current" />
              </span>
              <span aria-hidden>{watched ? "Play again" : "Play the launch film"}</span>
              <span aria-hidden className="font-mono text-xs text-white/70 tabular-nums">
                {length}
              </span>
            </span>
          </button>
        ) : null}

        {phase === "loading" ? (
          <div role="status" className={CORNER}>
            <span className={PILL}>
              <span aria-hidden className={cn(DISC, "bg-white/10")}>
                <LoaderCircleIcon className="size-5 motion-safe:animate-spin" />
              </span>
              Loading the film
            </span>
          </div>
        ) : null}

        {phase === "failed" ? (
          <div role="alert" className="absolute inset-x-0 bottom-0 bg-black/80 p-3 text-sm text-white">
            The film did not load.{" "}
            <a href={file} className="font-medium text-accent underline underline-offset-4">
              Open it as a file
            </a>
            .
          </div>
        ) : null}
      </div>

      <noscript>
        <video
          controls
          preload="none"
          playsInline
          poster={poster.jpg}
          aria-label={title}
          aria-describedby={describedBy}
          className="absolute inset-0 size-full bg-black object-contain"
        >
          {sources.map((source) => (
            <source key={source.src} src={source.src} type={source.type} />
          ))}
        </video>
      </noscript>
    </div>
  );
}
