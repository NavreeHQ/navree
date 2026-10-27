"use client";

/**
 * `useOperation(id)`: follows a long-running operation (an import, an export)
 * over its event stream through the backend-for-frontend.
 */
import {
  cancelOperation,
  followOperation,
  isFinished,
  type NavreeError,
  type Operation,
  type OperationFetch,
} from "@navree/client";
import { useCallback, useEffect, useRef, useState } from "react";

export interface UseOperationOptions {
  /** Sends the requests; default `fetch` on the app's own origin (the BFF adds the token). */
  fetch?: OperationFetch;
  /** Where the routes are (default `/api/operations`). */
  basePath?: string;
  /** Called once with the finished operation. */
  onFinished?: (operation: Operation) => void;
}

export interface UseOperationResult<Result> {
  /** The latest state, `undefined` until the first event. */
  operation: Operation<Result> | undefined;
  /** The error that stopped following (404, network, a closed stream). */
  error: NavreeError | Error | undefined;
  /** True once the operation succeeded, failed or was cancelled. */
  finished: boolean;
  /** Cancels the operation (`DELETE /api/operations/{id}`). */
  cancel: () => Promise<void>;
}

/** Follows operation `id` (or nothing, for `null`) and re-renders on every change. */
export function useOperation<Result = unknown>(
  id: string | null | undefined,
  options: UseOperationOptions = {},
): UseOperationResult<Result> {
  const [operation, setOperation] = useState<Operation<Result> | undefined>(undefined);
  const [error, setError] = useState<NavreeError | Error | undefined>(undefined);
  const optionsRef = useRef(options);
  optionsRef.current = options;

  useEffect(() => {
    setOperation(undefined);
    setError(undefined);
    if (!id) return;
    const controller = new AbortController();
    const { fetch, basePath } = optionsRef.current;
    followOperation<Result>(id, {
      fetch,
      basePath,
      signal: controller.signal,
      onUpdate: (next) => {
        if (!controller.signal.aborted) setOperation(next);
      },
    })
      .then((finished) => {
        if (!controller.signal.aborted) optionsRef.current.onFinished?.(finished as Operation);
      })
      .catch((reason: unknown) => {
        if (controller.signal.aborted) return;
        setError(reason instanceof Error ? reason : new Error(String(reason)));
      });
    return () => controller.abort();
  }, [id]);

  const cancel = useCallback(async () => {
    if (!id) return;
    const { fetch, basePath } = optionsRef.current;
    const cancelled = await cancelOperation<Result>(id, { fetch, basePath });
    setOperation(cancelled);
  }, [id]);

  return { operation, error, finished: operation !== undefined && isFinished(operation), cancel };
}
