"use client";

import { useEffect, useRef } from "react";

/** Records the command menu can create. */
export type CreateKind = "deal" | "company" | "contact" | "task";

const EVENT = "navree:create";
let pending: CreateKind | null = null;

/**
 * Asks the screen that owns `kind` to open its "New …" dialog. The command menu
 * navigates to that screen first: if it is already mounted the event opens the
 * dialog at once, otherwise the request waits until the screen mounts.
 */
export function requestCreate(kind: CreateKind): void {
  pending = kind;
  window.dispatchEvent(new CustomEvent<CreateKind>(EVENT, { detail: kind }));
}

/** Calls `onRequest` when the command menu asks to create a `kind` (one listener per kind and screen). */
export function useCreateIntent(kind: CreateKind | undefined, onRequest: () => void): void {
  const callback = useRef(onRequest);
  useEffect(() => {
    callback.current = onRequest;
  });
  useEffect(() => {
    if (!kind) return;
    const consume = () => {
      if (pending !== kind) return;
      pending = null;
      callback.current();
    };
    consume();
    const listener = () => consume();
    window.addEventListener(EVENT, listener);
    return () => window.removeEventListener(EVENT, listener);
  }, [kind]);
}
