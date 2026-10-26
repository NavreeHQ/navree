"use client";

import { useSyncExternalStore } from "react";

const QUERY = "(prefers-reduced-motion: reduce)";

function subscribe(onChange: () => void): () => void {
  const media = window.matchMedia(QUERY);
  media.addEventListener("change", onChange);
  return () => media.removeEventListener("change", onChange);
}

const onClient = () => window.matchMedia(QUERY).matches;
const onServer = () => false;

/**
 * True when the visitor asks for reduced motion, for components that render
 * different markup with it. The server, and the client while it hydrates,
 * both see `false`, so the markup matches and React reports no hydration
 * error; the client then re-renders with the real setting straight away.
 * (motion's own `useReducedMotion` reads the setting during the first client
 * render, which breaks hydration when the answer is yes.) Components whose
 * markup doesn't change can keep motion's hook.
 */
export function useReducedMotion(): boolean {
  return useSyncExternalStore(subscribe, onClient, onServer);
}
