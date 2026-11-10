/**
 * Real time in Vue: one connection per app, provided to components, and
 * `useChannel` to listen on a channel while a component lives.
 *
 * `@navree/nuxt/realtime` needs `pusher-js` (an optional peer dependency)
 * only when it connects with pusher-js; `createMemoryRealtime` needs nothing.
 */
import {
  connectRealtime,
  type ChannelRef,
  type ConnectRealtimeOptions,
  type RealtimeConnection,
} from "@navree/client/realtime";
import { inject, ref, shallowRef, toValue, watch, type App, type InjectionKey, type MaybeRefOrGetter, type Ref, type ShallowRef } from "vue";

export interface RealtimeController {
  connection: ShallowRef<RealtimeConnection | null>;
  state: Ref<string>;
  /** Opens the connection (pusher-js by default); failures leave the app working without real time. */
  connect(make?: () => RealtimeConnection | Promise<RealtimeConnection>): Promise<void>;
  disconnect(): void;
}

export const REALTIME_KEY: InjectionKey<RealtimeController> = Symbol("navree-realtime");

/** A controller for `server` (or for the connection `make` returns). */
export function createRealtimeController(options?: ConnectRealtimeOptions): RealtimeController {
  const connection = shallowRef<RealtimeConnection | null>(null);
  const state = ref("disconnected");
  let stopState: (() => void) | undefined;

  return {
    connection,
    state,
    async connect(make) {
      if (connection.value) return;
      try {
        const made = make ? await make() : connectRealtime(options as ConnectRealtimeOptions);
        stopState = made.onStateChange((next) => {
          state.value = next;
        });
        state.value = made.state;
        connection.value = made;
      } catch (error) {
        console.info("[navree] Real time disabled:", error);
      }
    },
    disconnect() {
      stopState?.();
      connection.value?.disconnect();
      connection.value = null;
      state.value = "disconnected";
    },
  };
}

/** Provides `controller` to the app's components (`app.use`-style). */
export function installRealtime(app: App, controller: RealtimeController): void {
  app.provide(REALTIME_KEY, controller);
}

/** The socket's state: `connecting`, `connected`, `unavailable`, `failed` or `disconnected`. */
export function useRealtimeState(): Ref<string> {
  return inject(REALTIME_KEY, null)?.state ?? ref("disconnected");
}

/**
 * Calls `handler` with every `event` on `channel` while the component lives.
 * A bare channel name is private (`crm.deals` → `private-crm.deals`); `null`
 * listens to nothing. The channel can be a ref or a getter.
 *
 * ```ts
 * useChannel("crm.deals", "deal.updated", () => queryClient.invalidateQueries({ queryKey: deals.all }));
 * ```
 */
export function useChannel<T = unknown>(
  channel: MaybeRefOrGetter<ChannelRef | null | undefined>,
  event: string,
  handler: (data: T) => void,
): void {
  const realtime = inject(REALTIME_KEY, null);
  if (!realtime) return;
  const key = () => {
    const value = toValue(channel);
    return value === null || value === undefined ? null : typeof value === "string" ? value : `public:${value.public}`;
  };
  watch(
    [() => realtime.connection.value, key],
    ([connection], _previous, onCleanup) => {
      const current = toValue(channel);
      if (!connection || current === null || current === undefined) return;
      onCleanup(connection.listen<T>(current, event, (data) => handler(data)));
    },
    { immediate: true },
  );
}
