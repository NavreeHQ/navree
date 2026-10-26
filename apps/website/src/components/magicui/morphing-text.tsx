"use client";

import { useCallback, useEffect, useId, useRef } from "react";

import { cn } from "@/lib/utils";
import { useReducedMotion } from "@/lib/use-reduced-motion";

const morphTime = 1.5;
const cooldownTime = 1.75;

const useMorphingText = (texts: string[], enabled: boolean) => {
  const textIndexRef = useRef(0);
  const morphRef = useRef(0);
  const cooldownRef = useRef(0);
  const timeRef = useRef(0);

  const text1Ref = useRef<HTMLSpanElement>(null);
  const text2Ref = useRef<HTMLSpanElement>(null);

  const setStyles = useCallback(
    (fraction: number) => {
      const [current1, current2] = [text1Ref.current, text2Ref.current];
      if (!current1 || !current2) return;

      current2.style.filter = `blur(${Math.min(8 / fraction - 8, 100)}px)`;
      current2.style.opacity = `${Math.pow(fraction, 0.4) * 100}%`;

      const invertedFraction = 1 - fraction;
      current1.style.filter = `blur(${Math.min(8 / invertedFraction - 8, 100)}px)`;
      current1.style.opacity = `${Math.pow(invertedFraction, 0.4) * 100}%`;

      current1.textContent = texts[textIndexRef.current % texts.length];
      current2.textContent = texts[(textIndexRef.current + 1) % texts.length];
    },
    [texts],
  );

  const doMorph = useCallback(() => {
    morphRef.current -= cooldownRef.current;
    cooldownRef.current = 0;

    let fraction = morphRef.current / morphTime;
    if (fraction > 1) {
      cooldownRef.current = cooldownTime;
      fraction = 1;
    }

    setStyles(fraction);
    if (fraction === 1) textIndexRef.current++;
  }, [setStyles]);

  const doCooldown = useCallback(() => {
    morphRef.current = 0;
    const [current1, current2] = [text1Ref.current, text2Ref.current];
    if (current1 && current2) {
      current2.style.filter = "none";
      current2.style.opacity = "100%";
      current1.style.filter = "none";
      current1.style.opacity = "0%";
    }
  }, []);

  useEffect(() => {
    if (!enabled) return;
    let animationFrameId: number;
    timeRef.current = performance.now();
    cooldownRef.current = cooldownTime;

    const animate = (now: number) => {
      animationFrameId = requestAnimationFrame(animate);
      const dt = (now - timeRef.current) / 1000;
      timeRef.current = now;
      cooldownRef.current -= dt;
      if (cooldownRef.current <= 0) doMorph();
      else doCooldown();
    };

    animationFrameId = requestAnimationFrame(animate);
    return () => cancelAnimationFrame(animationFrameId);
  }, [doMorph, doCooldown, enabled]);

  return { text1Ref, text2Ref };
};

interface MorphingTextProps {
  className?: string;
  texts: string[];
}

/**
 * Words that melt into each other (a gooey threshold filter). Use it for
 * one short word or phrase in a hero; it is block-level and centred by
 * default. Under prefers-reduced-motion the first text stays still.
 */
export function MorphingText({ texts, className }: MorphingTextProps) {
  const reduced = useReducedMotion();
  const { text1Ref, text2Ref } = useMorphingText(texts, !reduced);
  const filterId = `threshold-${useId().replace(/:/g, "")}`;

  return (
    <div
      className={cn(
        "relative mx-auto h-[1.1em] w-full text-center font-display leading-none font-semibold text-fg",
        className,
      )}
      style={{ filter: reduced ? undefined : `url(#${filterId}) blur(0.6px)` }}
    >
      <span className="sr-only">{texts.join(", ")}</span>
      <span aria-hidden className="absolute inset-x-0 top-0 m-auto inline-block w-full" ref={text1Ref} style={{ opacity: 0 }} />
      <span aria-hidden className="absolute inset-x-0 top-0 m-auto inline-block w-full" ref={text2Ref}>
        {texts[0]}
      </span>
      <svg aria-hidden className="fixed size-0" preserveAspectRatio="xMidYMid slice">
        <defs>
          <filter id={filterId}>
            <feColorMatrix
              in="SourceGraphic"
              type="matrix"
              values="1 0 0 0 0
                      0 1 0 0 0
                      0 0 1 0 0
                      0 0 0 255 -140"
            />
          </filter>
        </defs>
      </svg>
    </div>
  );
}
