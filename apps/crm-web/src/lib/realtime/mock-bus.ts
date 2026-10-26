import type { RealtimeConnection } from "./connection";

/** Mirrors `MOCK_BROADCAST_EVENT` in `src/mocks/handlers.ts` (kept separate so mocks never load in production). */
const MOCK_BROADCAST_EVENT = "navis-mock:broadcast";

/** In mock mode, the MSW handlers broadcast on a window event bus instead of a socket. */
export function connectMockBus(): RealtimeConnection {
  const listeners: { channel: string; handler: (e: Event) => void }[] = [];
  return {
    listen(channel, event, callback) {
      const handler = (e: Event) => {
        const detail = (e as CustomEvent<{ channel: string; event: string; data: unknown }>).detail;
        if (detail.channel === `private-${channel}` && detail.event === event) callback(detail.data);
      };
      window.addEventListener(MOCK_BROADCAST_EVENT, handler);
      listeners.push({ channel, handler });
    },
    leave(channel) {
      for (const l of listeners.filter((x) => x.channel === channel)) {
        window.removeEventListener(MOCK_BROADCAST_EVENT, l.handler);
      }
    },
    disconnect() {
      for (const l of listeners) window.removeEventListener(MOCK_BROADCAST_EVENT, l.handler);
      listeners.length = 0;
    },
  };
}
