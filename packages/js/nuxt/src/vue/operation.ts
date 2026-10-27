/** `useOperation(id)`: follows a long-running operation over its event stream. */
import { cancelOperation, followOperation, isFinished, type Operation, type OperationFetch } from "@navree/client";
import { computed, ref, shallowRef, toValue, watch, type ComputedRef, type MaybeRefOrGetter, type Ref, type ShallowRef } from "vue";

export interface UseOperationOptions {
  /** Sends the requests; default `fetch` on the app's own origin (the BFF adds the token). */
  fetch?: OperationFetch;
  /** Where the routes are (default `/api/operations`). */
  basePath?: string;
  /** Called once with the finished operation. */
  onFinished?: (operation: Operation) => void;
}

export interface UseOperationResult<Result> {
  operation: ShallowRef<Operation<Result> | undefined>;
  error: Ref<Error | undefined>;
  finished: ComputedRef<boolean>;
  cancel(): Promise<void>;
}

/** Follows operation `id` (a ref, getter or value; `null` follows nothing) while the component lives. */
export function useOperation<Result = unknown>(
  id: MaybeRefOrGetter<string | null | undefined>,
  options: UseOperationOptions = {},
): UseOperationResult<Result> {
  const operation = shallowRef<Operation<Result> | undefined>(undefined);
  const error = ref<Error | undefined>(undefined);

  watch(
    () => toValue(id),
    (current, _previous, onCleanup) => {
      operation.value = undefined;
      error.value = undefined;
      if (!current) return;
      const controller = new AbortController();
      onCleanup(() => controller.abort());
      followOperation<Result>(current, {
        fetch: options.fetch,
        basePath: options.basePath,
        signal: controller.signal,
        onUpdate: (next) => {
          if (!controller.signal.aborted) operation.value = next;
        },
      })
        .then((finished) => {
          if (!controller.signal.aborted) options.onFinished?.(finished as Operation);
        })
        .catch((reason: unknown) => {
          if (!controller.signal.aborted) error.value = reason instanceof Error ? reason : new Error(String(reason));
        });
    },
    { immediate: true },
  );

  return {
    operation,
    error,
    finished: computed(() => operation.value !== undefined && isFinished(operation.value)),
    async cancel() {
      const current = toValue(id);
      if (!current) return;
      operation.value = await cancelOperation<Result>(current, { fetch: options.fetch, basePath: options.basePath });
    },
  };
}
