import type { ChannelRef, RealtimeConnection } from "@navree/client/realtime";

/** Mirrors `MOCK_BROADCAST_EVENT` in `src/mocks/handlers.ts` (kept separate so mocks never load in production). */
const MOCK_BROADCAST_EVENT = "navree-mock:broadcast";

type BusEntry = { channel: string; handler: (event: Event) => void };

/** The protocol name of a channel, as the mock API broadcasts it: bare names are private. */
function protocolName(channel: ChannelRef): string {
  if (typeof channel !== "string") return channel.public;
  return channel.startsWith("private-") || channel.startsWith("presence-") ? channel : `private-${channel}`;
}

/**
 * In mock mode the MSW handlers broadcast on a window event bus instead of a socket. This is the
 * connection `RealtimeProvider` uses then: the same interface as the real one, listening on that bus.
 */
export function connectMockBus(): RealtimeConnection {
  const entries = new Set<BusEntry>();
  const watchers = new Set<(state: string) => void>();
  let state = "connected";

  const stop = (entry: BusEntry) => {
    window.removeEventListener(MOCK_BROADCAST_EVENT, entry.handler);
    entries.delete(entry);
  };

  return {
    listen<T>(channel: ChannelRef, event: string, callback: (data: T) => void) {
      const name = protocolName(channel);
      const entry: BusEntry = {
        channel: name,
        handler: (e) => {
          const detail = (e as CustomEvent<{ channel: string; event: string; data: unknown }>).detail;
          if (detail.channel === name && detail.event === event) callback(detail.data as T);
        },
      };
      window.addEventListener(MOCK_BROADCAST_EVENT, entry.handler);
      entries.add(entry);
      return () => stop(entry);
    },
    leave(channel) {
      const name = protocolName(channel);
      for (const entry of [...entries]) if (entry.channel === name) stop(entry);
    },
    disconnect() {
      for (const entry of [...entries]) stop(entry);
      state = "disconnected";
      for (const watcher of watchers) watcher(state);
    },
    get state() {
      return state;
    },
    onStateChange(callback) {
      watchers.add(callback);
      return () => {
        watchers.delete(callback);
      };
    },
  };
}
